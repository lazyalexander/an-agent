//! The host accepts a deployment made without Waver or a source analyzer.
use an_agent_host::execution::{self, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

struct Fixture {
    root: PathBuf,
    deployment: PathBuf,
    store: PathBuf,
    input: PathBuf,
}
impl Fixture {
    fn new() -> Result<Self> {
        let root = std::env::temp_dir().canonicalize()?.join(format!(
            "agent-execution-{}",
            an_agent_core::det_seam::Entropy::os().uuid_v4()
        ));
        fs::create_dir(&root)?;
        let deployment = root.join("deployment");
        fs::create_dir(&deployment)?;
        let source = root.join("main.rs");
        fs::write(
            &source,
            "use std::io::{self, Read}; fn main() { let mut s=String::new(); io::stdin().read_to_string(&mut s).unwrap(); if s.trim()==\"attempt-write\" { std::fs::write(\"forbidden.txt\", \"bad\").unwrap(); } print!(\"{}\", s.trim()); }",
        )?;
        assert!(
            Command::new("rustc")
                .arg(&source)
                .arg("-o")
                .arg(deployment.join("program"))
                .status()?
                .success()
        );
        let binary_hash = format!(
            "{:x}",
            Sha256::digest(fs::read(deployment.join("program"))?)
        );
        fs::write(
            deployment.join("spool.yaml"),
            format!(
                "v: 1\nkind: spool\nname: test-kit\nversion: 1.0.0\nsummary: Test CLI\nsort: beat\nconstructor: host\nhost: an-agent-stdio-v1\neffect:\n  file: {{ op: none }}\n  memory: {{ op: ignore }}\n  net: none\n  proc: spawn\n  model: {{ op: none }}\n  flow: both\ninverse: irreversible\nrequires: []\nconsumes: [kit.request]\nproduces: [kit.execute]\nconfig:\n  profile: stdio-only-v1\n  executable_sha256: \"{binary_hash}\"\n"
            ),
        )?;
        let input = root.join("input.txt");
        fs::write(&input, b"  hello kit  \n")?;
        Ok(Self {
            store: root.join("store"),
            root,
            deployment,
            input,
        })
    }
    fn release(&self) -> Result<String> {
        Ok(
            execution::register(&self.deployment, &self.store)?["registered"]
                .as_str()
                .ok_or("missing identity")?
                .into(),
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn runtime_contract_is_independent_and_refuses_tampering() -> Result<()> {
    let fixture = Fixture::new()?;
    let release = fixture.release()?;
    assert_eq!(fixture.release()?, release);
    let plan = execution::run(&fixture.store, &release, &fixture.input, false)?;
    assert_eq!(plan["environment"], serde_json::json!({}));
    assert_eq!(plan["context"]["trust"], "caller-supplied-data");
    assert!(!fixture.store.join("runs").exists());
    assert!(execution::run(&fixture.store, "../../escape", &fixture.input, true).is_err());
    fs::write(
        fixture
            .store
            .join("releases")
            .join(&release)
            .join("program"),
        b"changed",
    )?;
    assert!(execution::run(&fixture.store, &release, &fixture.input, true).is_err());
    assert!(!fixture.store.join("runs").exists());
    Ok(())
}

#[test]
fn contract_does_not_accept_extra_authority_or_ambiguous_config() -> Result<()> {
    let fixture = Fixture::new()?;
    let path = fixture.deployment.join("spool.yaml");
    let original = fs::read_to_string(&path)?;
    for changed in [
        original.replace("net: none", "net: egress"),
        original.replace("op: none", "op: unbounded"),
        original.replace("kit.execute", "kit.other"),
        original.replace("profile: stdio-only-v1", "profile: shell"),
        format!("{original}  command: arbitrary\n"),
    ] {
        fs::write(&path, changed)?;
        assert!(execution::check(&fixture.deployment).is_err());
    }
    fs::write(&path, &original)?;
    fixture.release()?;
    fs::write(&path, original.replace("Test CLI", "Changed metadata"))?;
    assert!(fixture.release().is_err());
    Ok(())
}

#[test]
fn input_bounds_and_links_are_rejected_before_execution() -> Result<()> {
    let fixture = Fixture::new()?;
    let release = fixture.release()?;
    fs::write(&fixture.input, vec![b'x'; 8193])?;
    assert!(execution::run(&fixture.store, &release, &fixture.input, true).is_err());
    #[cfg(unix)]
    {
        let link = fixture.root.join("linked");
        std::os::unix::fs::symlink(&fixture.deployment, &link)?;
        assert!(execution::check(&link).is_err());
    }
    assert!(!fixture.store.join("runs").exists());
    Ok(())
}

#[test]
fn actual_execution_retains_context_receipt_and_kernel_audit() -> Result<()> {
    let fixture = Fixture::new()?;
    let release = fixture.release()?;
    let result = execution::run(&fixture.store, &release, &fixture.input, true)?;
    let receipt_path = Path::new(result["receipt"].as_str().ok_or("missing receipt")?);
    let receipt: Value = serde_json::from_slice(&fs::read(receipt_path)?)?;
    let folder = receipt_path.parent().ok_or("missing run folder")?;
    assert_eq!(
        fs::read(folder.join("input.txt"))?,
        fs::read(&fixture.input)?
    );
    assert_eq!(receipt["release"], release);
    let audit = fs::read_to_string(folder.join("audit.json"))?;
    if cfg!(target_os = "macos") {
        assert_eq!(
            result["status"],
            "succeeded",
            "{}",
            fs::read_to_string(folder.join("stderr.txt"))?
        );
        assert_eq!(fs::read(folder.join("stdout.txt"))?, b"hello kit");
        assert!(audit.contains("applied"));
    } else {
        assert_eq!(result["status"], "refused");
        assert!(!audit.contains("\"applied\""));
    }
    assert!(audit.contains("spool"));
    // A retry is a new run; no silent caching or duplicate-success claim.
    let second = execution::run(&fixture.store, &release, &fixture.input, true)?;
    assert_ne!(result["receipt"], second["receipt"]);
    Ok(())
}

#[test]
#[cfg(target_os = "macos")]
fn undeclared_effect_is_denied_and_failure_never_becomes_applied() -> Result<()> {
    let fixture = Fixture::new()?;
    let release = fixture.release()?;
    fs::write(&fixture.input, b"attempt-write")?;
    let result = execution::run(&fixture.store, &release, &fixture.input, true)?;
    assert_eq!(result["status"], "failed");
    let receipt_path = Path::new(result["receipt"].as_str().ok_or("missing receipt")?);
    let folder = receipt_path.parent().ok_or("missing run")?;
    assert!(!folder.join("forbidden.txt").exists());
    assert!(!fs::read_to_string(folder.join("audit.json"))?.contains("\"applied\""));
    let receipt: Value = serde_json::from_slice(&fs::read(receipt_path)?)?;
    assert_ne!(receipt["exit_code"], 0);
    Ok(())
}
