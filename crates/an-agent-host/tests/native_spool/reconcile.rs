use super::*;
#[test]
fn interrupted_outer_acceptance_is_reconciled_without_execution_or_republication() -> Result<()> {
    let t = TempDir::new("spool-accept-reconcile");
    let base = t.path().canonicalize()?;
    let bundle = package(&base)?;
    let root = base.join("instance");
    spool::load(&bundle, &root)?;
    let input = base.join("input.txt");
    fs::write(&input, b"hello\n")?;
    run(&root, "run", &input, &AtomicBool::new(false))?;
    assert!(spool::reconcile_accept(&root, "run", None).is_err());
    let outer = root.join("runs/run/accepted.json");
    // Real filesystem failure after workspace publication, before facade recording.
    fs::create_dir(&outer)?;
    assert!(accept(&root, "run").is_err());
    fs::remove_dir(&outer)?;
    assert_eq!(
        spool::status(&root, "run")?["artifact_status"],
        "unconfirmed"
    );
    let attempts = root.join("runs/run/job/attempts");
    let attempt = fs::read_dir(&attempts)?.next().unwrap()?.path();
    let raw = fs::read(attempt.join("accepted.json"))?;
    let revisions = root.join("runs/run/job/artifacts/revisions");
    let count = fs::read_dir(&revisions)?.count();
    let execution = attempt.join("execution.json");
    let execution_bytes = fs::read(&execution)?;
    fs::write(&execution, b"changed")?;
    assert!(spool::reconcile_accept(&root, "run", None).is_err());
    assert!(!outer.exists());
    fs::write(&execution, execution_bytes)?;
    let plan = spool::reconcile_accept(&root, "run", None)?;
    assert!(!outer.exists());
    let result = spool::reconcile_accept(&root, "run", plan["approval_sha256"].as_str())?;
    assert_eq!(result["execution_retried"], false);
    assert_eq!(result["artifacts_republished"], false);
    let verified = spool::artifacts(&root, "run")?;
    assert_eq!(spool::status(&root, "run")?["artifact_status"], "accepted");
    assert_eq!(
        spool::reconcile_accept(&root, "run", plan["approval_sha256"].as_str())?["already_recorded"],
        true
    );
    assert_eq!(fs::read_dir(&attempts)?.count(), 1);
    assert_eq!(fs::read_dir(&revisions)?.count(), count);
    assert_eq!(fs::read(attempt.join("accepted.json"))?, raw);
    let saved = fs::read(&outer)?;
    fs::write(&outer, b"{}")?;
    assert!(spool::reconcile_accept(&root, "run", None).is_err());
    fs::write(&outer, saved)?;
    reclaim(&root, "run")?;
    unload(&root)?;
    assert_eq!(spool::artifacts(&root, "run")?, verified);
    Ok(())
}
