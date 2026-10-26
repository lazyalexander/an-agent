//! Fixed byte limits, independent custody and opt-in CLI diagnostics.
use super::*;
use an_agent_host::artifact_versions;
use std::process::Command;

fn cli(args: &[&str], timing: bool) -> Result<std::process::Output> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_an-agent-host"));
    command.args(args).env_remove("AN_AGENT_HOST_TIMING");
    if timing {
        command.env("AN_AGENT_HOST_TIMING", "1");
    }
    Ok(command.output()?)
}

#[test]
fn large_binary_custody_keeps_limits_and_timing_cannot_grant_a_lock() -> Result<()> {
    let t = TempDir::new("large-handoff");
    let (profile, root) = fixture(&t, "unused", 20)?;
    let source = t.path().canonicalize()?.join("selected/large.bin");
    fs::write(&source, vec![0u8; 8 * 1024 * 1024])?;
    let mut spec: Value = serde_json::from_slice(&fs::read(&profile)?)?;
    spec["args"] = json!([
        "-c",
        "cp \"$1\" a.bin; cp \"$1\" b.bin",
        "fixture",
        "{input:0}"
    ]);
    spec["inputs"] = json!([source]);
    spec["outputs"] = json!(["a.bin", "b.bin"]);
    spec["binary_outputs"] = spec["outputs"].clone();
    fs::write(&profile, serde_json::to_vec(&spec)?)?;
    let receipt = run(&profile, &root, None, &AtomicBool::new(false))?;
    assert_eq!(receipt["status"], "succeeded");
    let id = receipt["attempt"].as_str().unwrap();
    let path = root.to_str().unwrap();
    let args = ["workspace-accept", path, id];
    fs::write(root.join("host.lock"), b"test-owned competing operation")?;
    let blocked = cli(&args, true)?;
    assert!(!blocked.status.success());
    let traces: Vec<Value> = String::from_utf8(blocked.stderr)?
        .lines()
        .filter_map(|line| line.strip_prefix("AN_AGENT_TIMING="))
        .map(serde_json::from_str)
        .collect::<std::result::Result<_, _>>()?;
    assert!(
        traces
            .iter()
            .any(|m| m["stage"] == "lock_acquire" && m["outcome"] == "conflict")
    );
    assert!(!root.join("artifacts").exists());
    fs::remove_file(root.join("host.lock"))?;
    let quiet = cli(&args, false)?;
    assert!(quiet.status.success());
    assert!(!String::from_utf8(quiet.stderr)?.contains("AN_AGENT_TIMING="));
    let preview: Value = serde_json::from_slice(&quiet.stdout)?;
    let approved = preview["approval_sha256"].as_str().unwrap();
    let committed = cli(&["workspace-accept", path, id, "--approve", approved], true)?;
    assert!(committed.status.success());
    let accepted: Value = serde_json::from_slice(&committed.stdout)?;
    let traces: Vec<Value> = String::from_utf8(committed.stderr)?
        .lines()
        .filter_map(|line| line.strip_prefix("AN_AGENT_TIMING="))
        .map(serde_json::from_str)
        .collect::<std::result::Result<_, _>>()?;
    assert!(
        traces
            .iter()
            .any(|m| m["stage"] == "lock_hold" && m["outcome"] == "released")
    );
    assert!(
        traces
            .iter()
            .any(|m| m["stage"] == "write_sync" && m["bytes"] == 8 * 1024 * 1024)
    );
    let reclaim = job::reclaim(&root, id, None)?;
    job::reclaim(&root, id, reclaim["approval_sha256"].as_str())?;
    assert!(!root.join("work").exists());
    for (i, output) in accepted["outputs"].as_array().unwrap().iter().enumerate() {
        let target = t.path().canonicalize()?.join(format!("retained-{i}"));
        artifact_versions::checkout(
            &root.join("artifacts"),
            output["binding"]["revision"].as_str().unwrap(),
            &target,
        )?;
        let bytes = fs::read(target)?;
        assert_eq!(bytes.len(), 8 * 1024 * 1024);
        assert!(bytes.iter().all(|b| *b == 0));
    }
    Ok(())
}

#[test]
fn cancellation_during_silk_forking_does_not_wait_for_inherited_pipes() -> Result<()> {
    for _ in 0..32 {
        let t = TempDir::new("cancel-fork-race");
        let (profile, root) = fixture(
            &t,
            "printf ready > ready; for n in {1..16}; do /bin/sleep 30 & done; wait",
            10,
        )?;
        let stop = AtomicBool::new(false);
        let start = Instant::now();
        let receipt = thread::scope(|scope| -> Result<Value> {
            let watcher = scope.spawn(|| {
                while !root.join("work/ready").exists() && start.elapsed() < Duration::from_secs(5)
                {
                    thread::sleep(Duration::from_millis(1));
                }
                stop.store(true, Ordering::Relaxed);
                root.join("work/ready").exists()
            });
            let receipt = run(&profile, &root, None, &stop)?;
            assert!(watcher.join().unwrap());
            Ok(receipt)
        })?;
        assert!(start.elapsed() < Duration::from_secs(5), "{receipt}");
        assert_eq!(receipt["status"], "cancelled");
        assert_eq!(receipt["termination_confirmed"], true);
        assert_eq!(receipt["group_observation"]["empty"], true);
        let id = receipt["attempt"].as_str().unwrap();
        assert!(job::accept(&root, id, None).is_err());
        let reclaim = job::reclaim(&root, id, None)?;
        job::reclaim(&root, id, reclaim["approval_sha256"].as_str())?;
    }
    Ok(())
}
