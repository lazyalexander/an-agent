//! Provider fixtures exercise the real host path, not live SPACE qualification.
use super::*;
use an_agent_core::testkit::TempDir;
use std::{fs, sync::Arc, thread, time::Duration};
#[test]
fn space_uses_common_admission_acceptance_and_cancellation() -> Result<()> {
    let python = ["/opt/homebrew/bin/python3", "/usr/bin/python3"]
        .into_iter()
        .find(|p| Path::new(p).exists())
        .ok_or("Python required for SPACE transport tests")?;
    let python = Path::new(python).canonicalize()?;
    let temp = TempDir::new("space-host");
    let root = temp.path().canonicalize()?;
    let fixture = root.join("official-fixture.py");
    store::write_new(&fixture, include_bytes!("space_fixture.py"))?;
    let deployment = root.join("deployment");
    fs::create_dir(&deployment)?;
    let program = b"#!/bin/sh\nprintf accepted > output.txt\n";
    fs::write(deployment.join("program"), program)?;
    fs::write(
        deployment.join("spool.yaml"),
        format!(
            "v: 1\nkind: spool\nname: space-test\nversion: 1.0.0\nsummary: SPACE host test\nsort: beat\nconstructor: host\nhost: an-agent-file-v1\neffect:\n  file: {{ op: rw, path: /work, recursive: true }}\n  memory: {{ op: ignore }}\n  net: none\n  proc: spawn\n  model: {{ op: none }}\n  flow: both\ninverse: irreversible\nrequires: []\nconsumes: [kit.request]\nproduces: [kit.execute]\nconfig:\n  profile: file-kit-v1\n  executable_sha256: \"{}\"\n",
            store::digest(program)
        ),
    )?;
    let host = root.join("store");
    let registered = crate::execution::register(&deployment, &host)?;
    let release = registered["registered"].as_str().ok_or("release")?;
    let input = root.join("input");
    fs::write(&input, b"input")?;
    for (mode, expected) in [
        ("ok", "succeeded"),
        ("failed", "failed"),
        ("oversize", "failed"),
        ("foreign", "termination_unconfirmed"),
        ("changed", "termination_unconfirmed"),
        ("uncertain", "termination_unconfirmed"),
        ("unknown-delete", "termination_unconfirmed"),
        ("wrong-delete", "termination_unconfirmed"),
        ("cancel", "cancelled"),
    ] {
        let token = root.join(format!("token-{mode}"));
        store::write_new(&token, mode.as_bytes())?;
        let profile = root.join(format!("profile-{mode}"));
        store::write_new(
            &profile,
            &serde_json::to_vec(&json!({
            "python":python,"python_sha256":store::digest(&fs::read(&python)?),
            "client":fixture,"client_sha256":store::digest(include_bytes!("space_fixture.py")),
            "token_file":token,"api_url":"http://127.0.0.1:1","data_plane_url":"http://127.0.0.1:2",
            "template":format!("seed@sha256:{}","ab".repeat(32)),"control_seconds":30}))?,
        )?;
        let preview = crate::execution::run_space(
            &profile,
            &host,
            release,
            &input,
            false,
            &AtomicBool::new(false),
        )?;
        assert_eq!(preview["context"]["sha256"], store::digest(b"input"));
        assert_eq!(preview["execute"], false);
        let cancelled = Arc::new(AtomicBool::new(false));
        let watch = Arc::clone(&cancelled);
        let runs = host.join("runs");
        let watcher = if mode == "cancel" {
            Some(thread::spawn(move || {
                for _ in 0..1000 {
                    if fs::read_dir(&runs).is_ok_and(|mut entries| {
                        entries.any(|e| e.is_ok_and(|e| e.path().join("run-started").exists()))
                    }) {
                        watch.store(true, Ordering::Relaxed);
                        return;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                panic!("remote fixture never started");
            }))
        } else {
            None
        };
        let result =
            crate::execution::run_space(&profile, &host, release, &input, true, &cancelled)?;
        if let Some(w) = watcher {
            w.join().expect("cancellation watcher");
        }
        assert_eq!(result["status"], expected, "{mode}");
        let receipt_path = Path::new(result["receipt"].as_str().ok_or("receipt")?);
        let receipt: Value = serde_json::from_slice(&fs::read(receipt_path)?)?;
        assert_eq!(!receipt["artifact"].is_null(), mode == "ok");
        assert!(receipt_path.parent().unwrap().join("audit.json").exists());
        if mode == "ok" {
            assert_eq!(
                fs::read(receipt_path.parent().unwrap().join("artifact.txt"))?,
                b"accepted"
            );
        }
        if mode == "cancel" {
            assert_eq!(receipt["space"]["termination_confirmed"], true);
        }
    }
    assert_eq!(fs::read_dir(host.join("runs"))?.count(), 9);
    Ok(())
}
