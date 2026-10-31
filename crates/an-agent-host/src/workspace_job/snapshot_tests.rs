//! Fault windows are exercised without spawning a platform-specific runtime.
use super::*;
use an_agent_core::testkit::TempDir;

fn fixture() -> Result<(TempDir, Value, PathBuf)> {
    let temp = TempDir::new("input-snapshot-faults");
    let base = temp.path().canonicalize()?;
    let source = base.join("selected.txt");
    fs::write(&source, b"approved")?;
    let attempt = base.join("job/attempts/test");
    fs::create_dir_all(&attempt)?;
    let executable = std::env::current_exe()?;
    // Unit test binaries can exceed the runtime's executable size bound.
    let executable = if cfg!(unix) {
        PathBuf::from("/bin/echo").canonicalize()?
    } else {
        executable
    };
    let plan = json!({"schema":"an-workspace-plan-v2","root":base.join("job"),
        "executable":executable,"executable_sha256":store::digest(&regular(&executable,20*1024*1024)?),
        "inputs":[{"path":source,"name":"selected.txt","sha256":store::digest(b"approved")}],
        "args":["{input:0}"],"environment":{"SELECTED":"{input:0}"},"readonly":[]});
    Ok((temp, plan, attempt))
}

#[test]
fn preparation_refuses_changed_source_and_partial_snapshot_without_overwriting() -> Result<()> {
    for fault in ["source", "partial", "manifest", "cancelled"] {
        let (_temp, plan, attempt) = fixture()?;
        match fault {
            "source" => fs::write(plan["inputs"][0]["path"].as_str().unwrap(), b"changed")?,
            "partial" => {
                fs::create_dir_all(attempt.join("snapshot/inputs"))?;
                fs::write(attempt.join("snapshot/inputs/selected.txt"), b"partial")?;
            }
            "manifest" => fs::create_dir(attempt.join("execution.json"))?,
            _ => (),
        }
        assert!(prepare(&plan, &attempt, &AtomicBool::new(fault == "cancelled")).is_err());
        assert!(!attempt.join("execution.json").is_file());
        assert!(!attempt.join("intent.json").exists());
        if fault == "partial" {
            assert_eq!(
                fs::read(attempt.join("snapshot/inputs/selected.txt"))?,
                b"partial"
            );
        }
        assert!(verified(&attempt, &plan, &json!({})).is_err());
    }
    Ok(())
}

#[test]
fn persisted_snapshot_survives_source_removal_and_requires_exact_execution_binding() -> Result<()> {
    let (_temp, plan, attempt) = fixture()?;
    let execution = prepare(&plan, &attempt, &AtomicBool::new(false))?;
    assert_eq!(execution["args"][0], execution["environment"]["SELECTED"]);
    fs::remove_file(plan["inputs"][0]["path"].as_str().unwrap())?;
    let bytes = fs::read(attempt.join("execution.json"))?;
    let receipt = json!({"execution_sha256":store::digest(&bytes)});
    assert_eq!(verified(&attempt, &plan, &receipt)?, execution);
    assert!(verified(&attempt, &plan, &json!({})).is_err());
    let mut substituted = execution;
    substituted["args"] = json!(["/unapproved/path"]);
    let bytes = serde_json::to_vec(&substituted)?;
    fs::write(attempt.join("execution.json"), &bytes)?;
    assert!(
        verified(
            &attempt,
            &plan,
            &json!({"execution_sha256":store::digest(&bytes)})
        )
        .is_err()
    );
    let mut legacy = plan;
    legacy["schema"] = json!("an-workspace-plan-v1");
    assert!(launch(&legacy, &attempt).is_err());
    Ok(())
}

#[test]
fn artifact_snapshot_reads_the_pinned_revision_not_the_editable_checkout() -> Result<()> {
    let (temp, mut plan, attempt) = fixture()?;
    let catalog = temp.path().canonicalize()?.join("catalog");
    let published = crate::artifact_versions::publish_bytes(&catalog, b"approved", json!({}))?;
    let revision = published["revision"].as_str().unwrap();
    let (_, binding) = crate::artifact_versions::read(&catalog, revision)?;
    plan["artifact_inputs"] =
        json!([{"path":plan["inputs"][0]["path"], "catalog":catalog,"binding":binding}]);
    fs::write(
        plan["inputs"][0]["path"].as_str().unwrap(),
        b"edited after approval",
    )?;
    prepare(&plan, &attempt, &AtomicBool::new(false))?;
    assert_eq!(
        fs::read(attempt.join("snapshot/inputs/selected.txt"))?,
        b"approved"
    );
    Ok(())
}

#[test]
fn nested_layout_preserves_tree_and_rejects_aliases_and_prefix_collisions() -> Result<()> {
    let (_temp, mut plan, attempt) = fixture()?;
    plan["inputs"][0]["name"] = json!("project/kit/selected.txt");
    prepare(&plan, &attempt, &AtomicBool::new(false))?;
    assert_eq!(
        fs::read(attempt.join("snapshot/inputs/project/kit/selected.txt"))?,
        b"approved"
    );
    for name in [
        "../escape",
        "/absolute",
        "a//b",
        "a/./b",
        "a\\b",
        "{input:0}",
    ] {
        assert!(validate_name(name, &[]).is_err(), "{name}");
    }
    for (a, b) in [("a", "a"), ("a", "a/b"), ("a/b", "a")] {
        assert!(validate_name(b, &[json!({"name":a})]).is_err());
    }
    Ok(())
}

#[test]
fn nested_snapshot_refuses_existing_parent_symlink() -> Result<()> {
    let (temp, mut plan, attempt) = fixture()?;
    plan["inputs"][0]["name"] = json!("project/selected.txt");
    fs::create_dir_all(attempt.join("snapshot/inputs"))?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(temp.path(), attempt.join("snapshot/inputs/project"))?;
    #[cfg(unix)]
    assert!(prepare(&plan, &attempt, &AtomicBool::new(false)).is_err());
    Ok(())
}
