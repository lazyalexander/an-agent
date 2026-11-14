use super::*;
fn fixture() -> Result<(an_agent_core::testkit::TempDir, PathBuf, String, PathBuf)> {
    let temporary = an_agent_core::testkit::TempDir::new("product-task");
    let service = temporary.path().canonicalize()?.join("service");
    fs::create_dir_all(service.join("application/tasks"))?;
    let id = an_agent_core::det_seam::Entropy::os().uuid_v4().to_string();
    let root = directory(&service, &id)?;
    store::directory(&root)?;
    Ok((temporary, service, id, root))
}
#[test]
fn retained_creation_cannot_relaunch_or_change_its_content() -> Result<()> {
    let (_temporary, service, id, root) = fixture()?;
    let upload = an_agent_core::det_seam::Entropy::os().uuid_v4().to_string();
    let plan = json!({"id":id,"input":service.join("application/uploads").join(&upload),"instruction":"preserve source","environment_digest":"a".repeat(64)});
    store::write_new(&root.join("plan.json"), &serde_json::to_vec(&plan)?)?;
    let request = json!({"id":id,"upload":upload,"instruction":"preserve source","environment":"a".repeat(64),"approve":store::digest(&serde_json::to_vec(&plan)?)});
    assert_eq!(create(&service, &request)?["id"], id);
    assert!(!root.join("launch.json").exists());
    let mut changed = request;
    changed["instruction"] = json!("different task");
    assert!(create(&service, &changed).is_err());
    assert_eq!(fs::read_dir(&root)?.count(), 1);
    Ok(())
}
#[test]
fn clarification_is_exact_and_replay_does_not_write_new_answers() -> Result<()> {
    let (_temporary, service, id, root) = fixture()?;
    store::write_new(
        &root.join("state.json"),
        &serde_json::to_vec(
            &json!({"state":"waiting-clarification","questions":["currency","quantity:0"]}),
        )?,
    )?;
    let mut request = json!({"id":id,"answers":{"currency":"USD","quantity:0":12}});
    assert!(clarify(&service, &request).is_err());
    request["answers"]["currency"] = json!("EUR");
    let preview = clarify(&service, &request)?;
    request["approve"] = preview["approval_sha256"].clone();
    clarify(&service, &request)?;
    let bytes = fs::read(root.join("clarification.json"))?;
    assert_eq!(clarify(&service, &request)?["replayed"], false);
    assert_eq!(bytes, fs::read(root.join("clarification.json"))?);
    request["answers"]["quantity:0"] = json!(13);
    assert!(clarify(&service, &request).is_err());
    Ok(())
}
#[test]
fn product_download_never_reads_a_running_tasks_scratch() -> Result<()> {
    let (_temporary, service, id, root) = fixture()?;
    store::write_new(&root.join("state.json"), b"{\"state\":\"running\"}")?;
    assert!(artifact(&service, &id, "final").is_err());
    assert!(view(&service, "../other").is_err());
    Ok(())
}
#[test]
fn unknown_delivery_needs_original_process_and_vm_receipts() -> Result<()> {
    let (_temporary, service, id, root) = fixture()?;
    let session = an_agent_core::det_seam::Entropy::os().uuid_v4().to_string();
    let job = service.join("jobs").join(&session);
    fs::create_dir_all(&job)?;
    store::write_new(
        &root.join("state.json"),
        &serde_json::to_vec(&json!({"state":"unknown","session":session,"steps":{}}))?,
    )?;
    assert_eq!(reconcile(&service, &id)?["state"]["state"], "unknown");
    store::write_new(&job.join("completion.json"), b"{\"result\":{\"termination_confirmed\":true,\"cocoon\":{\"termination_confirmed\":false}}}")?;
    assert_eq!(reconcile(&service, &id)?["state"]["state"], "unknown");
    fs::write(job.join("completion.json"), b"{\"result\":{\"termination_confirmed\":true,\"cocoon\":{\"termination_confirmed\":true}}}")?;
    store::write_new(&root.join("cancel.json"), b"{}")?;
    let result = reconcile(&service, &id)?;
    assert_eq!(result["state"]["state"], "cancelled");
    assert_eq!(result["state"]["termination_confirmed"], true);
    assert_eq!(reconcile(&service, &id)?, result);
    assert!(!root.join("launch.json").exists());
    Ok(())
}
