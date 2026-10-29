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
    reclaim(&root, "run")?;
    unload(&root)?;
    let final_status = spool::status(&root, "run")?;
    assert_eq!(final_status["phase"], "reclaimed");
    assert_eq!(final_status["artifact_status"], "accepted");
    assert_eq!(final_status["outputs"], accepted["outputs"]);
    Ok(())
}
