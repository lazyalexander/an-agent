use super::*;
pub(super) fn fixture() -> Result<(an_agent_core::testkit::TempDir, PathBuf, String, PathBuf)> {
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

#[cfg(target_os = "macos")]
#[test]
fn runtime_cleanup_requires_exact_original_child_and_birth_identity() -> Result<()> {
    use crate::execution::sandbox::{product_process, service_process};
    let (_temporary, service, id, root) = fixture()?;
    let owner =
        json!({"pid":std::process::id(),"identity":service_process::identity(std::process::id())});
    store::write_new(&root.join("owner.json"), &serde_json::to_vec(&owner)?)?;
    assert!(runtime::record(&service, &id, std::process::id()).is_err());
    store::write_new(&root.join("runtime-launch.json"), b"{}")?;
    assert!(!runtime::cleanup(&root)?);
    let mut child = product_process::test_control()?;
    let result = (|| -> Result<()> {
        runtime::record(&service, &id, child.id())?;
        let path = root.join("runtime-owner.json");
        let original: Value = serde_json::from_slice(&fs::read(&path)?)?;
        let mut changed = original.clone();
        changed["identity"]["started_seconds"] = json!(0);
        fs::write(&path, serde_json::to_vec(&changed)?)?;
        assert!(runtime::cleanup(&root).is_err());
        assert!(child.try_wait()?.is_none());
        fs::write(&path, serde_json::to_vec(&original)?)?;
        Ok(())
    })();
    if let Err(error) = result {
        child.kill()?;
        child.wait()?;
        return Err(error);
    }
    let waiter = std::thread::spawn(move || child.wait());
    assert!(runtime::cleanup(&root)?);
    waiter.join().map_err(|_| "runtime reaper panicked")??;
    assert!(runtime::cleanup(&root)?);
    Ok(())
}

#[test]
fn preparation_loss_requires_version_and_original_start_authority() -> Result<()> {
    let (_temporary, service, id, root) = fixture()?;
    store::write_new(&root.join("state.json"), b"{\"state\":\"unknown\"}")?;
    assert_eq!(reconcile(&service, &id)?["state"]["state"], "unknown");
    store::write_new(&root.join("recovery-version.json"), b"{\"version\":1}")?;
    store::write_new(&root.join("start-intent.json"), b"{}")?;
    assert_eq!(reconcile(&service, &id)?["state"]["state"], "unknown");
    fs::remove_file(root.join("start-intent.json"))?;
    store::write_new(&root.join("runtime-launch.json"), b"{}")?;
    assert_eq!(reconcile(&service, &id)?["state"]["state"], "unknown");
    fs::remove_file(root.join("runtime-launch.json"))?;
    assert_eq!(reconcile(&service, &id)?["state"]["state"], "failed");
    assert!(!root.join("launch.json").exists());
    Ok(())
}

#[test]
fn unsupported_recovery_version_keeps_admission_blocked() -> Result<()> {
    let (_temporary, service, id, root) = fixture()?;
    store::write_new(&root.join("state.json"), b"{\"state\":\"unknown\"}")?;
    store::write_new(&root.join("recovery-version.json"), b"{\"version\":2}")?;
    let result = reconcile(&service, &id)?;
    assert_eq!(result["state"]["state"], "unknown");
    assert_eq!(
        result["state"]["reconciliation"]["reason"],
        "unsupported delivery recovery version"
    );
    assert_ne!(result["state"]["termination_confirmed"], true);
    Ok(())
}

#[test]
fn missing_launch_owner_and_live_pid_without_birth_proof_cannot_settle() -> Result<()> {
    let (_temporary, service, id, root) = fixture()?;
    store::write_new(&root.join("recovery-version.json"), b"{\"version\":1}")?;
    store::write_new(&root.join("launch.json"), b"{}")?;
    assert_eq!(view(&service, &id)?["state"]["state"], "unknown");
    assert_eq!(reconcile(&service, &id)?["state"]["state"], "unknown");
    store::write_new(&root.join("state.json"), b"{\"state\":\"unknown\"}")?;
    let owner = json!({"pid":std::process::id(),"identity":null});
    store::write_new(&root.join("owner.json"), &serde_json::to_vec(&owner)?)?;
    assert_eq!(reconcile(&service, &id)?["state"]["state"], "unknown");
    assert_ne!(view(&service, &id)?["state"]["termination_confirmed"], true);
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn original_page_reaps_its_exited_coordinator_child() -> Result<()> {
    use crate::execution::sandbox::{product_process, service_process};
    let child = product_process::test_control()?;
    let pid = child.id();
    product_process::reap(child)?;
    assert_eq!(unsafe { libc::kill(pid as i32, libc::SIGTERM) }, 0);
    let deadline =
        an_agent_core::det_seam::Clock::monotonic_now() + std::time::Duration::from_secs(2);
    while !service_process::exited(pid)
        && an_agent_core::det_seam::Clock::monotonic_now() < deadline
    {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(service_process::exited(pid));
    Ok(())
}

#[test]
fn incomplete_original_receipt_stays_unknown_until_it_is_readable() -> Result<()> {
    let (_temporary, service, id, root) = fixture()?;
    let session = an_agent_core::det_seam::Entropy::os().uuid_v4().to_string();
    let job = service.join("jobs").join(&session);
    fs::create_dir_all(&job)?;
    store::write_new(
        &root.join("state.json"),
        &serde_json::to_vec(&json!({"state":"unknown","session":session}))?,
    )?;
    store::write_new(&job.join("completion.json"), b"{")?;
    let pending = reconcile(&service, &id)?;
    assert_eq!(pending["state"]["state"], "unknown");
    assert_eq!(
        pending["state"]["reconciliation"]["reason"],
        "original primary receipt incomplete or unavailable"
    );
    fs::write(job.join("completion.json"), b"{\"result\":{\"termination_confirmed\":true,\"cocoon\":{\"termination_confirmed\":true}}}")?;
    assert_eq!(reconcile(&service, &id)?["state"]["state"], "failed");
    assert!(!root.join("launch.json").exists());
    Ok(())
}
