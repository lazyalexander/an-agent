//! Deterministic boundaries use the real workspace runner and native supervisor.
use super::*;
use an_agent_core::testkit::TempDir;
use std::fs;

fn fixture() -> Result<(TempDir, PathBuf, PathBuf)> {
    let temp = TempDir::new("workspace-prelaunch");
    let base = temp.path().canonicalize()?;
    let selected = base.join("selected.txt");
    fs::write(&selected, b"approved")?;
    let profile = base.join("profile.json");
    fs::write(
        &profile,
        serde_json::to_vec(&json!({"schema":"an-workspace-job-v2",
        "executable":"/bin/bash","args":["-c","cat \"$1\" > output.txt","fixture","{input:0}"],
        "inputs":[selected],"readonly":[],"environment":{},"outputs":["output.txt"],
        "wall_seconds":10,"trusted_process_group":true}))?,
    )?;
    Ok((temp, profile, base.join("job")))
}

fn approved(profile: &Path, root: &Path, prior: Option<&str>, stop: &AtomicBool) -> Result<Value> {
    let preview = run(profile, root, prior, None, stop)?;
    run(
        profile,
        root,
        prior,
        preview["approval_sha256"].as_str(),
        stop,
    )
}

#[test]
fn preset_cancellation_creates_neither_workspace_nor_successor() -> Result<()> {
    let (_temp, profile, root) = fixture()?;
    let stop = AtomicBool::new(true);
    let cancelled = approved(&profile, &root, None, &stop)?;
    assert_eq!(cancelled["status"], "cancelled");
    assert_eq!(cancelled["process_started"], false);
    assert!(cancelled.get("attempt").is_none());
    assert!(!root.exists());
    stop.store(false, Ordering::Release);
    let prior = approved(&profile, &root, None, &stop)?;
    assert_eq!(prior["status"], "succeeded");
    let id = prior["attempt"].as_str().unwrap();
    stop.store(true, Ordering::Release);
    let cancelled = approved(&profile, &root, Some(id), &stop)?;
    assert!(cancelled.get("attempt").is_none());
    assert!(!attempt(&root, id)?.join("successor.json").exists());
    assert_eq!(fs::read_dir(root.join("attempts"))?.count(), 1);
    Ok(())
}

#[test]
fn positively_unstarted_partial_snapshots_can_be_collected_without_retry() -> Result<()> {
    for interrupted in [false, true] {
        let (_temp, profile, root) = fixture()?;
        let stop = AtomicBool::new(false);
        let selected = profile.parent().unwrap().join("selected.txt");
        let prior = profile.parent().unwrap().join("prior.txt");
        fs::write(&prior, b"first copied input")?;
        let mut spec: Value = serde_json::from_slice(&fs::read(&profile)?)?;
        spec["inputs"] = json!([prior, selected]);
        fs::write(&profile, serde_json::to_vec(&spec)?)?;
        let preview = run(&profile, &root, None, None, &stop)?;
        let receipt = run_inner(
            &profile,
            &root,
            None,
            preview["approval_sha256"].as_str(),
            &stop,
            |boundary, _| {
                if matches!(boundary, not_started::Boundary::IntentPersisted) {
                    fs::write(&selected, b"changed after approval").unwrap();
                }
            },
        )?;
        assert_eq!(receipt["schema"], "an-workspace-not-started-v1");
        assert_eq!(receipt["process_started"], false);
        let id = receipt["attempt"].as_str().unwrap();
        let dir = attempt(&root, id)?;
        assert!(!dir.join("launch.json").exists());
        assert!(reclaim_snapshots_managed(&root, id, None).is_err());
        let work = reclaim(&root, id, None)?;
        reclaim(&root, id, work["approval_sha256"].as_str())?;
        let inputs = dir.join("snapshot/inputs");
        let held = dir.join("snapshot/held-inputs");
        let outside = profile.parent().unwrap().join("outside");
        fs::create_dir(&outside)?;
        fs::rename(&inputs, &held)?;
        std::os::unix::fs::symlink(&outside, &inputs)?;
        assert!(reclaim_snapshots_managed(&root, id, None).is_err());
        assert!(outside.exists());
        fs::remove_file(&inputs)?;
        fs::rename(&held, &inputs)?;
        let snapshot = reclaim_snapshots_managed(&root, id, None)?;
        assert!(dir.join("snapshot/inputs/prior.txt").exists());
        if interrupted {
            store::write_new(
                &dir.join("snapshot-reclaim-intent.json"),
                &serde_json::to_vec(&snapshot["plan"])?,
            )?;
            fs::remove_file(dir.join("snapshot/inputs/prior.txt"))?;
            let before = fs::read_dir(dir.join("snapshot/inputs"))?.count();
            let observed = reclaim_snapshots_managed(&root, id, None)?;
            assert_eq!(observed["status"], "uncertain");
            assert_eq!(observed["executed"], false);
            assert!(dir.join("snapshot/inputs").exists());
            assert_eq!(fs::read_dir(dir.join("snapshot/inputs"))?.count(), before);
            assert!(!dir.join("snapshot-reclaimed.json").exists());
            continue;
        }
        reclaim_snapshots_managed(&root, id, snapshot["approval_sha256"].as_str())?;
        assert!(!dir.join("snapshot/inputs").exists());
        assert_eq!(
            reclaim_snapshots_managed(&root, id, None)?["status"],
            "succeeded"
        );
        assert_eq!(fs::read_dir(root.join("attempts"))?.count(), 1);
        assert!(dir.join("plan.json").exists());
    }
    Ok(())
}

#[test]
fn cancellation_at_both_durable_boundaries_can_resume_or_reclaim_but_never_publish() -> Result<()> {
    for before_snapshot in [true, false] {
        for resume in [true, false] {
            let (_temp, profile, root) = fixture()?;
            let stop = AtomicBool::new(false);
            let preview = run(&profile, &root, None, None, &stop)?;
            let receipt = run_inner(
                &profile,
                &root,
                None,
                preview["approval_sha256"].as_str(),
                &stop,
                |boundary, _| {
                    if (before_snapshot
                        && matches!(boundary, not_started::Boundary::IntentPersisted))
                        || (!before_snapshot
                            && matches!(boundary, not_started::Boundary::ReadyToLaunch))
                    {
                        stop.store(true, Ordering::Release);
                    }
                },
            )?;
            assert_eq!(receipt["schema"], "an-workspace-not-started-v1");
            assert_eq!(receipt["status"], "cancelled");
            assert_eq!(
                receipt["stage"],
                if before_snapshot {
                    "preparation"
                } else {
                    "supervisor"
                }
            );
            assert_eq!(receipt["process_started"], false);
            assert_eq!(receipt["reaped"], false);
            assert_eq!(receipt["pid"], Value::Null);
            let id = receipt["attempt"].as_str().unwrap();
            assert!(!root.join("work/output.txt").exists());
            assert!(accept(&root, id, None).is_err());
            assert!(!root.join("artifacts").exists());
            stop.store(false, Ordering::Release);
            if resume {
                let next = approved(&profile, &root, Some(id), &stop)?;
                assert_eq!(next["status"], "succeeded");
                assert_eq!(fs::read(root.join("work/output.txt"))?, b"approved");
                assert!(reclaim(&root, id, None).is_err());
            } else {
                let preview = reclaim(&root, id, None)?;
                reclaim(&root, id, preview["approval_sha256"].as_str())?;
                assert!(!root.join("work").exists());
                assert!(attempt(&root, id)?.join("receipt.json").exists());
            }
        }
    }
    Ok(())
}

#[test]
fn preparation_failure_settles_the_successor_and_retains_accepted_artifacts() -> Result<()> {
    let (temp, profile, root) = fixture()?;
    let stop = AtomicBool::new(false);
    let prior = approved(&profile, &root, None, &stop)?;
    let id = prior["attempt"].as_str().unwrap();
    let p = accept(&root, id, None)?;
    let accepted = accept(&root, id, p["approval_sha256"].as_str())?;
    let preview = run(&profile, &root, Some(id), None, &stop)?;
    let selected = temp.path().canonicalize()?.join("selected.txt");
    let failed = run_inner(
        &profile,
        &root,
        Some(id),
        preview["approval_sha256"].as_str(),
        &stop,
        |boundary, _| {
            if matches!(boundary, not_started::Boundary::IntentPersisted) {
                fs::write(&selected, b"changed after approval").unwrap();
            }
        },
    )?;
    assert_eq!(failed["status"], "failed");
    assert_eq!(failed["process_started"], false);
    assert_eq!(failed["stage"], "preparation");
    let next = failed["attempt"].as_str().unwrap();
    assert!(record(&root, id).is_err());
    assert!(accept(&root, next, None).is_err());
    assert_eq!(fs::read(root.join("work/output.txt"))?, b"approved");
    let p = reclaim(&root, next, None)?;
    reclaim(&root, next, p["approval_sha256"].as_str())?;
    let revision = accepted["outputs"][0]["binding"]["revision"]
        .as_str()
        .unwrap();
    let (bytes, _) = crate::artifact_versions::read(&root.join("artifacts"), revision)?;
    assert_eq!(bytes, b"approved");
    Ok(())
}

#[test]
fn missing_or_contradictory_not_started_evidence_is_not_settlement() -> Result<()> {
    let (_temp, profile, root) = fixture()?;
    let stop = AtomicBool::new(false);
    let preview = run(&profile, &root, None, None, &stop)?;
    let receipt = run_inner(
        &profile,
        &root,
        None,
        preview["approval_sha256"].as_str(),
        &stop,
        |boundary, _| {
            if matches!(boundary, not_started::Boundary::ReadyToLaunch) {
                stop.store(true, Ordering::Release);
            }
        },
    )?;
    let id = receipt["attempt"].as_str().unwrap();
    let dir = attempt(&root, id)?;
    let path = dir.join("receipt.json");
    let original = fs::read(&path)?;
    for field in [
        "process_started",
        "pid",
        "exit_code",
        "reaped",
        "termination_confirmed",
        "group_cleanup_required",
        "stage",
        "intent_sha256",
    ] {
        let mut invalid = receipt.clone();
        invalid.as_object_mut().unwrap().remove(field);
        fs::write(&path, serde_json::to_vec(&invalid)?)?;
        assert!(reclaim(&root, id, None).is_err(), "{field}");
        assert!(run(&profile, &root, Some(id), None, &AtomicBool::new(false)).is_err());
    }
    for (field, value) in [
        ("schema", json!("an-workspace-receipt-v1")),
        ("pid", json!(123)),
        ("reaped", json!(true)),
        ("status", json!("succeeded")),
        ("process_started", json!(true)),
        ("group_observation", json!({"empty":true})),
    ] {
        let mut invalid = receipt.clone();
        invalid[field] = value;
        fs::write(&path, serde_json::to_vec(&invalid)?)?;
        assert!(reclaim(&root, id, None).is_err(), "{field}");
    }
    fs::remove_file(&path)?;
    assert!(reclaim(&root, id, None).is_err());
    fs::write(&path, &original)?;
    fs::remove_file(dir.join("intent.json"))?;
    assert!(reclaim(&root, id, None).is_err());
    assert!(!root.join("reclaim-intent.json").exists());
    assert!(!dir.join("successor.json").exists());
    Ok(())
}

#[test]
fn launch_error_or_receipt_write_failure_does_not_authorize_cleanup() -> Result<()> {
    for receipt_failure in [false, true] {
        let (_temp, profile, root) = fixture()?;
        let stop = AtomicBool::new(false);
        let preview = run(&profile, &root, None, None, &stop)?;
        let mut id = String::new();
        let result = run_inner(
            &profile,
            &root,
            None,
            preview["approval_sha256"].as_str(),
            &stop,
            |boundary, dir| {
                if matches!(boundary, not_started::Boundary::ReadyToLaunch) {
                    id = dir.file_name().unwrap().to_str().unwrap().to_owned();
                    if receipt_failure {
                        fs::remove_file(dir.join("receipt.json")).unwrap();
                        fs::create_dir(dir.join("receipt.json")).unwrap();
                        stop.store(true, Ordering::Release);
                    } else {
                        // A spawn/pre-exec error is not positive no-spawn evidence.
                        fs::rename(root.join("work"), root.join("retained-work")).unwrap();
                    }
                }
            },
        );
        if receipt_failure {
            assert!(result.is_err());
        } else {
            let receipt = result?;
            assert_eq!(receipt["schema"], "an-workspace-receipt-v1");
            assert_eq!(receipt["termination_confirmed"], false);
            assert_eq!(receipt["status"], "refused");
        }
        assert!(reclaim(&root, &id, None).is_err());
        assert!(run(&profile, &root, Some(&id), None, &AtomicBool::new(false)).is_err());
        assert!(!root.join("reclaim-intent.json").exists());
    }
    Ok(())
}
