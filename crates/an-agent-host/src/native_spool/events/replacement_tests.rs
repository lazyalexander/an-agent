use super::*;
use an_agent_core::testkit::TempDir;

#[test]
fn one_catalog_commit_cuts_over_without_erasing_history_and_refuses_changed_prefix() -> Result<()> {
    let temp = TempDir::new("native-route-cutover");
    let root = temp.path().canonicalize()?.join("events");
    super::super::init(&root)?;
    let old_space = Space::open(&root)?;
    let previous = json!({"schema":"an-native-event-subscription-v1","workspace":old_space.id,
        "name":"old","source":"storage-fixture","output":"report.txt","kind":"artifact.accepted"});
    old_space
        .facts
        .put_config("native.events.old", &serde_json::to_vec(&previous)?)?;
    let payload = json!({"workspace":old_space.id,"kind":"artifact.accepted","fixture":true});
    let old_event = json!({"id":hash(&payload)?,"event":payload});
    old_space.append(&old_event)?;
    let prefix = old_space.facts.log()?;
    let successor = json!({"schema":"an-native-event-subscription-v2","workspace":old_space.id,
        "name":"next","source":"storage-fixture","output":"report.txt","kind":"artifact.accepted",
        "replaces":{"name":"old","subscription":hash(&previous)?},
        "cutover":{"records":prefix.len(),"prefix_sha256":store::digest(&serde_json::to_vec(&prefix)?)}});
    old_space.upgrade(3)?;
    old_space
        .facts
        .put_config("native.events.next", &serde_json::to_vec(&successor)?)?;
    let space = Space::open(&root)?;
    assert_eq!(space.route("old")?, previous);
    assert_eq!(space.route("next")?, successor);
    assert_eq!(
        retired_by(&space, &previous)?.ok_or("retirement")?["name"],
        "next"
    );
    assert!(eligible(&space, &old_event, &successor).is_err());
    assert!(old_space.route("next").is_err());
    old_space.upgrade(2)?; // A stale handle must never downgrade the metadata.
    assert_eq!(Space::open(&root)?.version, 3);
    let payload = json!({"workspace":space.id,"kind":"artifact.accepted","fixture":"later"});
    let new_event = json!({"id":hash(&payload)?,"event":payload});
    space.append(&new_event)?;
    assert!(eligible(&space, &new_event, &successor).is_ok());
    let mut changed = successor.clone();
    changed["cutover"]["prefix_sha256"] = json!("0".repeat(64));
    assert!(validate(&space, &changed).is_err());
    let mut records = space.facts.log()?;
    records[1].body = Some("changed historic fact".into());
    let bytes = records
        .iter()
        .map(serde_json::to_string)
        .collect::<std::result::Result<Vec<_>, _>>()?
        .join("\n")
        + "\n";
    fs::write(root.join("facts/log.jsonl"), bytes)?;
    assert!(space.route("next").is_err());
    assert!(not_retired(&space, &previous).is_err());
    Ok(())
}
