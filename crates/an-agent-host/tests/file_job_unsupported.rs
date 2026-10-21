//! An unsupported native file backend must retain refusal, never run directly.
#![cfg(not(target_os = "macos"))]
use an_agent_core::testkit::TempDir;
use an_agent_host::execution;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

#[test]
fn file_profile_refuses_without_an_unrestricted_fallback() -> execution::Result<()> {
    let temp = TempDir::new("unsupported-file-kit");
    let root = temp.path().canonicalize()?;
    let deployment = root.join("deployment");
    fs::create_dir(&deployment)?;
    let program = b"must never be executed";
    fs::write(deployment.join("program"), program)?;
    fs::write(
        deployment.join("spool.yaml"),
        format!(
            "v: 1\nkind: spool\nname: file-probe\nsummary: Unsupported profile probe\nversion: 0.1.0\nsort: beat\nconstructor: host\nhost: an-agent-file-v1\neffect:\n  file: {{ op: rw, path: /work, recursive: true }}\n  memory: {{ op: ignore }}\n  net: none\n  proc: spawn\n  model: {{ op: none }}\n  flow: both\ninverse: irreversible\nrequires: []\nconsumes: [kit.request]\nproduces: [kit.execute]\nconfig:\n  profile: file-kit-v1\n  executable_sha256: \"{:x}\"\n",
            Sha256::digest(program)
        ),
    )?;
    let store = root.join("store");
    let registered = execution::register(&deployment, &store)?;
    let release = registered["registered"].as_str().ok_or("missing release")?;
    let input = root.join("input.txt");
    fs::write(&input, "data")?;
    assert_eq!(
        execution::run(&store, release, &input, false)?["supported"],
        false
    );
    let result = execution::run(&store, release, &input, true)?;
    assert_eq!(result["status"], "refused");
    let receipt_path = Path::new(result["receipt"].as_str().ok_or("missing receipt")?);
    let receipt: serde_json::Value = serde_json::from_slice(&fs::read(receipt_path)?)?;
    assert!(receipt["process"]["pid"].is_null());
    let run = receipt_path.parent().ok_or("missing run")?;
    assert!(!run.join("artifact.txt").exists());
    assert!(!fs::read_to_string(run.join("audit.json"))?.contains("applied"));
    Ok(())
}
