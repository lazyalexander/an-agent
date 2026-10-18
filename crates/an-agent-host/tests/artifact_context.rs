use an_agent_core::testkit::TempDir;
use an_agent_host::{artifact_context, context_exchange::ContextPacket, execution::Result};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
use uuid::Uuid;

fn fixture(run: &Path, bytes: &[u8]) -> Result<()> {
    fs::create_dir(run)?;
    fs::write(run.join("stdout.txt"), bytes)?;
    fs::write(
        run.join("receipt.json"),
        json!({"schema_version":1,"status":"failed",
        "stdout":{"path":"stdout.txt","sha256":format!("{:x}",Sha256::digest(bytes))}})
        .to_string(),
    )?;
    Ok(())
}

#[test]
fn retains_provenance_budget_and_failure_status_without_mutating_source() -> Result<()> {
    let temp = TempDir::new("artifact-context");
    let root = temp.path().canonicalize()?;
    let run = root.join("run");
    let original = format!("START\n{}\nFINAL ERROR", "详细诊断".repeat(1000));
    fixture(&run, original.as_bytes())?;
    let receipt = fs::read(run.join("receipt.json"))?;
    let export = root.join("view");
    let result = artifact_context::export(&run, "stdout", Uuid::nil(), 1024, &export)?;
    assert_eq!(result["model_called"], false);
    let packet = fs::read(export.join("context.json"))?;
    let context = ContextPacket::parse(&packet, Uuid::nil())?;
    assert!(context.source.starts_with("host-artifact:failed:"));
    assert!(context.text.starts_with("START\n"));
    assert!(context.text.ends_with("FINAL ERROR"));
    assert!(context.text.len() <= 1024);
    assert!(packet.len() <= 4096);
    assert!(ContextPacket::parse(&packet, Uuid::from_u128(1)).is_err());
    let selection: serde_json::Value =
        serde_json::from_slice(&fs::read(export.join("selection.json"))?)?;
    let retained: u64 = selection["ranges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["end"].as_u64().unwrap() - r["start"].as_u64().unwrap())
        .sum();
    assert_eq!(
        retained + selection["omitted_bytes"].as_u64().unwrap(),
        original.len() as u64
    );
    assert_eq!(
        selection["packet_sha256"],
        format!("{:x}", Sha256::digest(&packet))
    );
    assert_eq!(fs::read(run.join("stdout.txt"))?, original.as_bytes());
    assert_eq!(fs::read(run.join("receipt.json"))?, receipt);
    let again = root.join("again");
    artifact_context::export(&run, "stdout", Uuid::nil(), 1024, &again)?;
    assert_eq!(packet, fs::read(again.join("context.json"))?);
    assert!(artifact_context::export(&run, "stdout", Uuid::nil(), 1024, &export).is_err());
    assert!(
        artifact_context::export(&run, "stdout", Uuid::nil(), 1024, &run.join("view")).is_err()
    );
    assert!(!run.join("view").exists());
    Ok(())
}

#[test]
fn rejects_tampering_paths_binary_and_escaped_packet_overflow_before_writes() -> Result<()> {
    let temp = TempDir::new("artifact-context-invalid");
    let root = temp.path().canonicalize()?;
    let run = root.join("run");
    fixture(&run, b"original")?;
    let output = root.join("view");
    for stream in ["../stdout", "/etc/passwd", "model"] {
        assert!(artifact_context::export(&run, stream, Uuid::nil(), 128, &output).is_err());
    }
    fs::write(run.join("stdout.txt"), b"changed")?;
    assert!(artifact_context::export(&run, "stdout", Uuid::nil(), 128, &output).is_err());
    for (name, data) in [
        ("binary", vec![255]),
        ("escaped", vec![0; 2048]),
        ("oversize", vec![b'x'; 65537]),
    ] {
        let source = root.join(name);
        fixture(&source, &data)?;
        assert!(artifact_context::export(&source, "stdout", Uuid::nil(), 2048, &output).is_err());
        assert!(!output.exists());
    }
    #[cfg(unix)]
    {
        let source = root.join("linked");
        fixture(&source, b"original")?;
        fs::remove_file(source.join("stdout.txt"))?;
        std::os::unix::fs::symlink(run.join("stdout.txt"), source.join("stdout.txt"))?;
        assert!(artifact_context::export(&source, "stdout", Uuid::nil(), 128, &output).is_err());
    }
    assert!(!output.exists());
    Ok(())
}
