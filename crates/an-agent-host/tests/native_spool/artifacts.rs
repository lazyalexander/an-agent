use super::*;
#[test]
fn formal_versions_survive_collection_and_unload_but_reject_foreign_records() -> Result<()> {
    let temp = TempDir::new("spool-artifacts");
    let base = temp.path().canonicalize()?;
    let bundle = package(&base)?;
    let a = base.join("a");
    let b = base.join("b");
    spool::load(&bundle, &a)?;
    spool::load(&bundle, &b)?;
    let input = base.join("input.txt");
    fs::write(&input, b"hello\n")?;
    run(&a, "one", &input, &AtomicBool::new(false))?;
    assert!(spool::artifacts(&a, "one").is_err());
    accept(&a, "one")?;
    let before = spool::artifacts(&a, "one")?;
    reclaim(&a, "one")?;
    unload(&a)?;
    assert_eq!(spool::artifacts(&a, "one")?, before);
    let output = base.join("checkout.txt");
    an_agent_host::artifact_versions::checkout(
        Path::new(before["catalog"].as_str().unwrap()),
        before["outputs"][0]["binding"]["revision"]
            .as_str()
            .unwrap(),
        &output,
    )?;
    assert_eq!(fs::read(&output)?, b"Report\nHELLO\n");
    run(&b, "one", &input, &AtomicBool::new(false))?;
    accept(&b, "one")?;
    let record = a.join("runs/one/accepted.json");
    let original = fs::read(&record)?;
    fs::copy(b.join("runs/one/accepted.json"), &record)?;
    assert!(spool::artifacts(&a, "one").is_err());
    fs::write(&record, original)?;
    let revision = before["outputs"][0]["binding"]["revision"]
        .as_str()
        .unwrap();
    let content = a
        .join("runs/one/job/artifacts/revisions")
        .join(revision)
        .join("content.txt");
    fs::write(content, b"tampered")?;
    assert!(spool::artifacts(&a, "one").is_err());
    Ok(())
}
