use super::*;
#[test]
fn stores_are_environment_scoped_and_reject_traversal() -> Result<()> {
    let service = Path::new("/private/task-host");
    assert_ne!(
        state_root(service, &"a".repeat(64))?,
        state_root(service, &"b".repeat(64))?
    );
    assert!(state_root(service, "../legacy").is_err());
    assert!(operation_root(service, &"a".repeat(64), "../other").is_err());
    Ok(())
}
#[test]
fn legacy_runtime_has_no_persistent_image_authority() -> Result<()> {
    let temporary = an_agent_core::testkit::TempDir::new("image-contract");
    let root = temporary.path().canonicalize()?;
    assert!(!capable(&root, &json!({"components":{}}))?);
    store::write_new(
        &root.join("contract.json"),
        br#"{"schema":"cocoon-image-api-v1","disk_images":true,"memory_restore_qualified":true}"#,
    )?;
    assert!(capable(&root, &json!({"components":{"image_api":"contract.json"}})).is_err());
    Ok(())
}
#[test]
fn retained_image_submission_cannot_relaunch_or_change_approval() -> Result<()> {
    let temporary = an_agent_core::testkit::TempDir::new("image-idempotency");
    let service = temporary.path().canonicalize()?;
    fs::create_dir_all(service.join("application"))?;
    let environment = "a".repeat(64);
    let id = an_agent_core::det_seam::Entropy::os().uuid_v4().to_string();
    let root = operation_root(&service, &environment, &id)?;
    fs::create_dir_all(&root)?;
    let plan = json!({"id":id,"action":{"operation":"branch"}});
    store::write_new(&root.join("plan.json"), &serde_json::to_vec(&plan)?)?;
    let receipt = json!({"id":id,"state":"succeeded","receipt":{"request":id,"state":"succeeded"}});
    store::write_new(&root.join("settled.json"), &serde_json::to_vec(&receipt)?)?;
    let mut request = json!({"id":id,"environment":environment,"approve":store::digest(&serde_json::to_vec(&plan)?)});
    assert_eq!(submit(&service, &request)?, receipt);
    assert!(!root.join("launch.json").exists());
    request["approve"] = json!("0".repeat(64));
    assert!(submit(&service, &request).is_err());
    Ok(())
}

#[test]
fn branch_settlement_uses_the_existing_runtime_manifest_limit() -> Result<()> {
    let temporary = an_agent_core::testkit::TempDir::new("large-image-manifest");
    let root = temporary.path().canonicalize()?;
    let manifest = json!({"files":{"metadata":"x".repeat(200_000)}});
    store::write_new(&root.join("runtime.json"), &serde_json::to_vec(&manifest)?)?;
    assert!(read(&root.join("runtime.json")).is_err());
    assert_eq!(runtime_manifest(&root)?, manifest);
    Ok(())
}
