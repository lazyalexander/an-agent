use super::*;
use an_agent_core::testkit::TempDir;

#[test]
fn selection_shape_cannot_duplicate_delivery_by_changing_state() -> Result<()> {
    let temp = TempDir::new("dispatch-selections");
    let path = temp.path().canonicalize()?.join("request.json");
    let first =
        json!({"event":"a".repeat(64),"subscription":"counter","state_revision":"b".repeat(64)});
    let mut second = first.clone();
    second["state_revision"] = json!("c".repeat(64));
    let mut value = json!({"schema":"an-native-event-dispatch-request-v1","name":"selected","entries":[first,second]});
    fs::write(&path, serde_json::to_vec(&value)?)?;
    assert!(Request::read(&path).is_err());
    value["entries"].as_array_mut().ok_or("entries")?.pop();
    fs::write(&path, serde_json::to_vec(&value)?)?;
    assert!(Request::read(&path).is_ok());
    value["entries"][0]["state_revision"] = json!("latest");
    fs::write(&path, serde_json::to_vec(&value)?)?;
    assert!(Request::read(&path).is_err());
    value["entries"] = json!([]);
    fs::write(&path, serde_json::to_vec(&value)?)?;
    assert!(Request::read(&path).is_err());
    Ok(())
}

#[test]
fn missing_entry_evidence_cannot_inherit_a_success_summary() -> Result<()> {
    let temp = TempDir::new("dispatch-evidence");
    let root = temp.path().canonicalize()?.join("events");
    super::super::init(&root)?;
    let space = Space::open(&root)?;
    store::directory(&root.join("dispatches"))?;
    let dir = root.join("dispatches/reviewed");
    store::directory(&dir)?;
    let plan = json!({"workspace":space.id,"request":{"name":"reviewed"},"entries":[{},{}]});
    let digest = hash(&plan)?;
    write_json(
        &dir.join("intent.json"),
        &json!({"schema":"an-native-event-dispatch-intent-v1",
        "plan":plan,"approval_sha256":digest}),
    )?;
    write_json(
        &dir.join("entry-1.json"),
        &json!({"dispatch_sha256":digest,"index":1}),
    )?;
    write_json(
        &dir.join("result.json"),
        &json!({"schema":"an-native-event-dispatch-result-v1",
        "dispatch_sha256":digest,"status":"succeeded","attempted_entries":2,"selected_entries":2}),
    )?;
    let observed = inspect_dispatch(&root, "reviewed")?;
    assert_eq!(observed["status"], "uncertain");
    assert_eq!(observed["reason"], "incomplete-entry-evidence");
    assert_eq!(observed["executed"], false);
    assert_eq!(observed["automatic_retries"], 0);
    Ok(())
}
