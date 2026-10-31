use super::*;
use an_agent_core::testkit::TempDir;

#[test]
fn control_chain_preserves_identity_and_rejects_old_schema_duplicate_and_tamper() -> Result<()> {
    let temp = TempDir::new("subscription-control");
    let root = temp.path().canonicalize()?.join("events");
    super::super::init(&root)?;
    let old = Space::open(&root)?;
    let route = json!({"name":"fixture-only"});
    let before = current(&old, &route)?;
    let after = successor(&before)?;
    let plan = transition(&before, &after);
    upgrade(&old)?;
    let space = Space::open(&root)?;
    let stored = json!({"schema":"an-native-subscription-control-record-v1",
        "approval_sha256":hash(&plan)?,"plan":plan});
    let name = format!("{PREFIX}{}.1", hash(&route)?);
    let record = space
        .facts
        .put_config(&name, &serde_json::to_vec(&stored)?)?;
    assert_eq!(current(&space, &route)?, after);
    assert!(current(&old, &route).is_err());
    // A duplicate committed record must not be collapsed into one transition.
    let log = root.join("facts/log.jsonl");
    let original = fs::read(&log)?;
    use std::io::Write;
    let mut file = fs::OpenOptions::new().append(true).open(&log)?;
    file.write_all(serde_json::to_string(&record)?.as_bytes())?;
    file.write_all(b"\n")?;
    assert!(current(&space, &route).is_err());
    fs::write(&log, original)?;
    let snapshot = root
        .join("facts/config")
        .join(record.sha256.ok_or("snapshot")?);
    fs::write(snapshot, b"{}")?;
    assert!(current(&space, &route).is_err());
    Ok(())
}
