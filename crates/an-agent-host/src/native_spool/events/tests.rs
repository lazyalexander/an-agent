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

#[test]
fn monitor_is_read_only_and_cancellation_never_dispatches() -> Result<()> {
    let (_temp, space) = fixture()?;
    let before = fs::read(space.root.join("facts/log.jsonl"))?;
    let flag = AtomicBool::new(false);
    let result = watch(&space.root, 30, 100, &flag, |value| {
        assert_eq!(value["deliveries"], json!([]));
        flag.store(true, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    })?;
    assert_eq!(result["reason"], "cancelled");
    assert_eq!(result["snapshots"], 1);
    assert_eq!(result["execution_cancelled"], false);
    assert_eq!(before, fs::read(space.root.join("facts/log.jsonl"))?);
    assert_eq!(fs::read_dir(space.root.join("deliveries"))?.count(), 0);
    assert!(watch(&space.root, 0, 100, &flag, |_| Ok(())).is_err());
    assert!(watch(&space.root, 1, 99, &flag, |_| Ok(())).is_err());
    Ok(())
}

#[test]
fn monitor_read_and_output_faults_stop_without_repair() -> Result<()> {
    let (_temp, space) = fixture()?;
    let flag = AtomicBool::new(false);
    assert!(
        watch(
            &space.root,
            1,
            100,
            &flag,
            |_| Err("closed observer".into())
        )
        .is_err()
    );
    fs::OpenOptions::new()
        .append(true)
        .open(space.root.join("facts/log.jsonl"))?
        .write_all(b"{torn")?;
    let mut emitted = false;
    assert!(
        watch(&space.root, 1, 100, &flag, |_| {
            emitted = true;
            Ok(())
        })
        .is_err()
    );
    assert!(!emitted);
    assert_eq!(fs::read(space.root.join("facts/log.jsonl"))?, b"{torn");
    Ok(())
}
