use super::*;
#[test]
fn status_uses_settlement_and_custody_evidence_not_marker_presence() -> Result<()> {
    let t = TempDir::new("spool-status");
    let base = t.path().canonicalize()?;
    let package = package(&base)?;
    let root = base.join("instance");
    spool::load(&package, &root)?;
    let input = base.join("input.txt");
    fs::write(&input, b"hello\n")?;
    assert!(spool::status(&root, "missing").is_err());
    spool::start(&root, "run", &input, 30, None, &AtomicBool::new(false))?;
    assert_eq!(spool::status(&root, "run")?["phase"], "prepared");
    run(&root, "run", &input, &AtomicBool::new(false))?;
    let settled = spool::status(&root, "run")?;
    assert_eq!(settled["phase"], "settled");
    assert_eq!(settled["termination_confirmed"], true);
    assert_eq!(settled["artifact_status"], "awaiting-acceptance");
    let result = root.join("runs/run/result.json");
    let bytes = fs::read(&result)?;
    let mut invalid: Value = serde_json::from_slice(&bytes)?;
    invalid["workspace"]["termination_confirmed"] = json!(false);
    fs::write(&result, serde_json::to_vec(&invalid)?)?;
    assert_eq!(spool::status(&root, "run")?["termination_confirmed"], false);
    fs::write(&result, bytes)?;
    // A forged outer collection marker cannot establish workspace collection.
    fs::write(root.join("runs/run/reclaimed.json"), b"{}")?;
    assert_eq!(spool::status(&root, "run")?["phase"], "settled");
    fs::remove_file(root.join("runs/run/reclaimed.json"))?;
    accept(&root, "run")?;
    let accepted = spool::status(&root, "run")?;
    assert_eq!(accepted["phase"], "accepted");
    // A stale operator lock is not missing artifact evidence. Observation must
    // neither acquire nor remove it, while custody operations still refuse.
    let lock = root.join("host.lock");
    fs::write(&lock, b"retained stale operation lock\n")?;
    let observed = spool::status(&root, "run");
    let custody_refused = spool::artifacts(&root, "run").is_err();
    let reclaim_refused = spool::reclaim(&root, "run", None).is_err();
    let held = fs::read(&lock);
    fs::remove_file(&lock)?;
    assert_eq!(observed?, accepted);
    assert!(custody_refused && reclaim_refused);
    assert_eq!(held?, b"retained stale operation lock\n");
    reclaim(&root, "run")?;
    unload(&root)?;
    let final_status = spool::status(&root, "run")?;
    assert_eq!(final_status["phase"], "reclaimed");
    assert_eq!(final_status["artifact_status"], "accepted");
    assert_eq!(final_status["outputs"], accepted["outputs"]);
    Ok(())
}

#[test]
#[allow(clippy::disallowed_methods, reason = "real process readiness deadline")]
fn accepted_status_remains_observable_during_another_run_in_the_same_instance() -> Result<()> {
    let temp = TempDir::new("spool-status-during-run");
    let base = temp.path().canonicalize()?;
    let package = package(&base)?;
    let root = base.join("instance");
    spool::load(&package, &root)?;
    let input = base.join("input.txt");
    fs::write(&input, b"hello\n")?;
    run(&root, "accepted", &input, &AtomicBool::new(false))?;
    accept(&root, "accepted")?;
    let expected = spool::status(&root, "accepted")?;
    let hold = base.join("hold.txt");
    fs::write(&hold, b"hold")?;
    let stop = AtomicBool::new(false);
    let (ready, observations, artifacts_refused, reclaim_refused, lock_unchanged, result) =
        thread::scope(|scope| {
            let worker =
                scope.spawn(|| run(&root, "running", &hold, &stop).map_err(|e| e.to_string()));
            let since = Instant::now();
            let witness = root.join("runs/running/job/work/ready");
            while !witness.exists()
                && !worker.is_finished()
                && since.elapsed() < Duration::from_secs(15)
            {
                thread::sleep(Duration::from_millis(10));
            }
            let ready = witness.exists() && !worker.is_finished();
            let before = fs::read(root.join("host.lock"));
            let observations = (0..3)
                .map(|_| spool::status(&root, "accepted"))
                .collect::<Vec<_>>();
            let artifacts_refused = spool::artifacts(&root, "accepted").is_err();
            let reclaim_refused = spool::reclaim(&root, "running", None).is_err();
            let lock_unchanged =
                before.is_ok() && before.ok() == fs::read(root.join("host.lock")).ok();
            // Always request cancellation and join before any assertion can fail.
            stop.store(true, Ordering::Release);
            (
                ready,
                observations,
                artifacts_refused,
                reclaim_refused,
                lock_unchanged,
                worker.join(),
            )
        });
    let result = result
        .expect("host worker panicked")
        .map_err(std::io::Error::other)?;
    assert!(ready);
    assert_eq!(result["status"], "cancelled");
    assert_eq!(result["workspace"]["termination_confirmed"], true);
    assert!(artifacts_refused && reclaim_refused && lock_unchanged);
    for observation in observations {
        assert_eq!(observation?, expected);
    }
    assert!(!root.join("host.lock").exists());
    assert_eq!(
        spool::artifacts(&root, "accepted")?["outputs"],
        expected["outputs"]
    );
    reclaim(&root, "running")?;
    reclaim(&root, "accepted")?;
    unload(&root)?;
    Ok(())
}
