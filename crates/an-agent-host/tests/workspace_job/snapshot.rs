//! Real reads must use approved snapshot bytes, not mutable selected paths.
use super::*;

#[test]
fn source_changes_during_execution_do_not_change_inputs_or_acceptance() -> Result<()> {
    let temp = TempDir::new("workspace-snapshot");
    let (profile, root) = fixture(&temp, "unused", 15)?;
    let selected = temp.path().canonicalize()?.join("selected");
    let source = selected.join("input.txt");
    let script = selected.join("run.sh");
    fs::write(
        &script,
        format!(
            "printf ready > ready; while [ ! -f go ]; do /bin/sleep 0.01; done; if cat '{}' >/dev/null 2>&1; then exit 93; fi; if printf overwrite > \"$1\" 2>/dev/null; then exit 94; fi; cat \"$1\" > output.txt",
            source.display()
        ),
    )?;
    let mut p: Value = serde_json::from_slice(&fs::read(&profile)?)?;
    p["args"] = json!(["{input:1}", "{input:0}"]);
    p["inputs"] = json!([source, script]);
    fs::write(&profile, serde_json::to_vec(&p)?)?;
    let stop = AtomicBool::new(false);
    let receipt = thread::scope(|scope| -> Result<Value> {
        let running = scope.spawn(|| run(&profile, &root, None, &stop).map_err(|e| e.to_string()));
        let start = Instant::now();
        while !root.join("work/ready").exists() {
            assert!(!running.is_finished(), "execution ended before ready");
            assert!(start.elapsed() < Duration::from_secs(10));
            thread::sleep(Duration::from_millis(10));
        }
        fs::write(&source, b"unapproved")?;
        fs::write(root.join("work/go"), b"go")?;
        Ok(running.join().unwrap().map_err(std::io::Error::other)?)
    })?;
    assert_eq!(receipt["status"], "succeeded");
    assert_eq!(fs::read(root.join("work/output.txt"))?, b"immutable");
    assert_eq!(receipt["status"], "succeeded");
    let id = receipt["attempt"].as_str().unwrap();
    let attempt = root.join("attempts").join(id);
    let execution: Value = serde_json::from_slice(&fs::read(attempt.join("execution.json"))?)?;
    assert!(
        execution["args"]
            .as_array()
            .unwrap()
            .iter()
            .all(|arg| !arg.as_str().unwrap().contains(selected.to_str().unwrap()))
    );
    assert_eq!(
        fs::read(attempt.join("snapshot/inputs/input.txt"))?,
        b"immutable"
    );
    // Acceptance reopens persisted host evidence after both original files vanish.
    fs::remove_dir_all(&selected)?;
    let preview = job::accept(&root, id, None)?;
    job::accept(&root, id, preview["approval_sha256"].as_str())?;
    let reclaim = job::reclaim(&root, id, None)?;
    job::reclaim(&root, id, reclaim["approval_sha256"].as_str())?;
    assert!(attempt.join("snapshot/inputs/input.txt").exists());
    assert!(root.join("artifacts").exists());
    Ok(())
}

#[test]
fn unsafe_profiles_and_missing_snapshot_authority_are_refused() -> Result<()> {
    let temp = TempDir::new("snapshot-refusal");
    let (profile, root) = fixture(&temp, "printf result > output.txt", 10)?;
    let original: Value = serde_json::from_slice(&fs::read(&profile)?)?;
    for change in ["legacy", "raw", "unknown", "runtime"] {
        let mut p = original.clone();
        match change {
            "legacy" => p["schema"] = json!("an-workspace-job-v1"),
            "raw" => p["args"] = json!([p["inputs"][0]]),
            "unknown" => p["args"] = json!(["{input:99}"]),
            _ => {
                p["readonly"] = json!([Path::new(p["inputs"][0].as_str().unwrap())
                    .parent()
                    .unwrap()])
            }
        }
        fs::write(&profile, serde_json::to_vec(&p)?)?;
        assert!(job::run(&profile, &root, None, None, &AtomicBool::new(false)).is_err());
        assert!(!root.exists());
    }
    fs::write(&profile, serde_json::to_vec(&original)?)?;
    let receipt = run(&profile, &root, None, &AtomicBool::new(false))?;
    assert_eq!(receipt["status"], "succeeded");
    let id = receipt["attempt"].as_str().unwrap();
    let path = root.join("attempts").join(id).join("execution.json");
    let bytes = fs::read(&path)?;
    fs::remove_file(&path)?;
    assert!(job::accept(&root, id, None).is_err());
    fs::write(&path, bytes)?;
    let snapshot = root
        .join("attempts")
        .join(id)
        .join("snapshot/inputs/input.txt");
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&snapshot, fs::Permissions::from_mode(0o600))?;
    fs::write(snapshot, b"tampered")?;
    assert!(job::accept(&root, id, None).is_err());
    assert!(!root.join("artifacts").exists());
    Ok(())
}
