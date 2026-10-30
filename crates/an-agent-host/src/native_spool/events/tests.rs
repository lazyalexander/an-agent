use super::*;
use an_agent_core::testkit::TempDir;
use std::io::Write;

fn fixture() -> Result<(TempDir, Space)> {
    let temp = TempDir::new("native-event-facts");
    let root = temp.path().canonicalize()?.join("events");
    init(&root)?;
    Ok((temp, Space::open(&root)?))
}
fn envelope(space: &Space) -> Result<Value> {
    let payload = json!({"schema":"an-native-artifact-event-v1","workspace":space.id,
        "kind":"artifact.accepted","fixture":"storage-only; not artifact authorization"});
    Ok(json!({"id":hash(&payload)?,"event":payload}))
}
#[test]
fn repeated_facts_confirm_without_appending_and_duplicates_fail_closed() -> Result<()> {
    let (_temp, space) = fixture()?;
    let event = envelope(&space)?;
    space.append(&event)?;
    let before = fs::read(space.root.join("facts/log.jsonl"))?;
    space.append(&event)?;
    assert_eq!(before, fs::read(space.root.join("facts/log.jsonl"))?);
    // Inject a conflicting log condition, not a second publication API.
    space.facts.push_event(
        native::event_contract::ARTIFACT,
        &serde_json::to_string(&event)?,
    )?;
    let duplicate = fs::read(space.root.join("facts/log.jsonl"))?;
    assert!(space.append(&event).is_err());
    assert_eq!(duplicate, fs::read(space.root.join("facts/log.jsonl"))?);
    Ok(())
}
#[test]
fn incomplete_workspace_append_is_neither_missing_nor_repaired() -> Result<()> {
    let (_temp, space) = fixture()?;
    let event = envelope(&space)?;
    space.append(&event)?;
    let log = space.root.join("facts/log.jsonl");
    fs::OpenOptions::new()
        .append(true)
        .open(&log)?
        .write_all(b"{torn")?;
    let before = fs::read(&log)?;
    assert!(space.append(&event).is_err());
    assert!(Space::open(&space.root).is_err());
    assert_eq!(before, fs::read(log)?);
    Ok(())
}
#[test]
fn foreign_workspace_identity_cannot_be_republished() -> Result<()> {
    let (_temp, space) = fixture()?;
    let mut event = envelope(&space)?;
    event["event"]["workspace"] = json!("foreign");
    event["id"] = json!(hash(&event["event"])?);
    assert!(space.append(&event).is_err());
    assert!(space.facts.log()?.is_empty());
    Ok(())
}
