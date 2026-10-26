//! Host proposal admission and pinned products, with no model or trusted fake runner.
use super::*;

fn proposal(base: &Path, request: &Value, code: &str) -> Result<PathBuf> {
    let path = base.join("proposal.json");
    fs::write(
        &path,
        serde_json::to_vec(&json!({
            "schema":"an-weaver-proposal-v1","request":request["request"]["request"],
            "request_sha256":request["request_sha256"],"kit":request["request"]["kit"],"content":code
        }))?,
    )?;
    Ok(path)
}
fn apply(root: &Path, path: &Path) -> Result<Value> {
    let p = spool::weaver::apply(root, path, None)?;
    spool::weaver::apply(root, path, p["approval_sha256"].as_str())
}

#[test]
fn proposal_slot_rejects_stale_revoked_foreign_and_protected_edits() -> Result<()> {
    let temp = TempDir::new("weaver-admission");
    let base = temp.path().canonicalize()?;
    let bundle = package(&base)?;
    let a = base.join("a");
    let b = base.join("b");
    spool::load(&bundle, &a)?;
    spool::load(&bundle, &b)?;
    assert!(spool::weaver::request(&a, "../silk/main.sh").is_err());
    assert!(spool::weaver::request(&a, "report.sh").is_err());
    let r = spool::weaver::request(&a, "transform.sh")?;
    let p = proposal(&base, &r, "printf adapted\n")?;
    assert!(apply(&b, &p).is_err());
    let mut invalid: Value = serde_json::from_slice(&fs::read(&p)?)?;
    invalid["kit"] = json!("silk/main.sh");
    fs::write(&p, serde_json::to_vec(&invalid)?)?;
    assert!(apply(&a, &p).is_err());
    let p = proposal(&base, &r, "printf adapted\n")?;
    let preview = spool::weaver::apply(&a, &p, None)?;
    proposal(&base, &r, "printf substituted\n")?;
    assert!(spool::weaver::apply(&a, &p, preview["approval_sha256"].as_str()).is_err());
    let revoked = spool::weaver::revoke(&a, r["request"]["request"].as_str().unwrap())?;
    assert_eq!(revoked["external_termination_confirmed"], false);
    assert!(apply(&a, &p).is_err());
    let r = spool::weaver::request(&a, "transform.sh")?;
    let p = proposal(&base, &r, "printf adapted\n")?;
    // Even an edit restoring identical bytes invalidates the prior generation.
    let source = bundle.join("kit/transform.sh");
    let edit = spool::update_kit(&a, "transform.sh", &source, None)?;
    spool::update_kit(
        &a,
        "transform.sh",
        &source,
        edit["approval_sha256"].as_str(),
    )?;
    assert!(apply(&a, &p).is_err());
    let r = spool::weaver::request(&a, "transform.sh")?;
    let p = proposal(&base, &r, "printf adapted\n")?;
    let input = base.join("input.txt");
    fs::write(&input, "input")?;
    run(&a, "baseline", &input, &AtomicBool::new(false))?;
    assert!(apply(&a, &p).is_err());
    spool::start(&a, "previewed", &input, 30, None, &AtomicBool::new(false))?;
    let r = spool::weaver::request(&a, "transform.sh")?;
    let p = proposal(&base, &r, "printf adapted\n")?;
    run(&a, "previewed", &input, &AtomicBool::new(false))?;
    assert!(apply(&a, &p).is_err());
    let r = spool::weaver::request(&a, "transform.sh")?;
    let p = proposal(&base, &r, "printf adapted\n")?;
    apply(&a, &p)?;
    assert!(apply(&a, &p).is_err());
    assert_eq!(
        fs::read(a.join("release/kit/transform.sh"))?,
        fs::read(source)?
    );
    Ok(())
}

#[test]
fn optional_proposal_and_fixed_revision_handoff_survive_reload_and_new_outputs() -> Result<()> {
    let temp = TempDir::new("spool-pinned-handoff");
    let base = temp.path().canonicalize()?;
    let bundle = package(&base)?;
    let a = base.join("a");
    let b = base.join("b");
    spool::load(&bundle, &a)?;
    spool::load(&bundle, &b)?;
    let r = spool::weaver::request(&a, "transform.sh")?;
    let p = proposal(&base, &r, "printf 'adapted:'; cat \"$1\"\n")?;
    apply(&a, &p)?;
    let input = base.join("input.txt");
    fs::write(&input, "hello\n")?;
    run(&a, "old", &input, &AtomicBool::new(false))?;
    let accepted = accept(&a, "old")?;
    let revision = accepted["result"]["outputs"][0]["binding"]["revision"]
        .as_str()
        .unwrap();
    let pinned = base.join("pinned.txt");
    an_agent_host::artifact_versions::checkout(
        &a.join("runs/old/job/artifacts"),
        revision,
        &pinned,
    )?;
    fs::write(&input, "new\n")?;
    run(&a, "new", &input, &AtomicBool::new(false))?;
    // An old execution receipt cannot claim a newer run's candidate.
    let path = a.join("runs/new/result.json");
    let original = fs::read(&path)?;
    fs::write(&path, fs::read(a.join("runs/old/result.json"))?)?;
    assert!(spool::accept(&a, "new", None).is_err());
    fs::write(&path, original)?;
    accept(&a, "new")?;
    run(&b, "consume", &pinned, &AtomicBool::new(false))?;
    assert_eq!(
        fs::read_to_string(b.join("runs/consume/job/work/report.txt"))?,
        "Report\nREPORT\nADAPTED:HELLO\n"
    );
    reclaim(&a, "old")?;
    reclaim(&a, "new")?;
    unload(&a)?;
    let fresh = base.join("fresh");
    spool::load(&bundle, &fresh)?;
    assert_eq!(
        fs::read(fresh.join("kit/transform.sh"))?,
        fs::read(bundle.join("kit/transform.sh"))?
    );
    assert_eq!(fs::read_to_string(pinned)?, "Report\nadapted:hello\n");
    Ok(())
}
