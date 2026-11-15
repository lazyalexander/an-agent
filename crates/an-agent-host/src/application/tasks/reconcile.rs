//! Rebuild only a settled projection from original owner receipts; never resume execution.
use super::*;

pub(in crate::application) fn reconcile(service: &Path, id: &str) -> Result<Value> {
    let observed = view(service, id)?;
    if observed["state"]["state"] != "unknown" {
        return Ok(observed);
    }
    let root = directory(service, id)?;
    let owner = root.join("owner.json");
    // A launch fence without an owner cannot prove that its coordinator is gone.
    if !owner.exists() && root.join("launch.json").exists() {
        return Ok(observed);
    }
    if owner.exists() {
        let owner: Value = serde_json::from_slice(&store::read(&owner, 65536)?)?;
        let pid = owner["pid"]
            .as_u64()
            .filter(|p| *p > 1 && *p <= i32::MAX as u64)
            .and_then(|p| u32::try_from(p).ok())
            .ok_or("original owner")?;
        // Missing birth metadata (including a reused live PID) is not exit proof.
        if !crate::execution::sandbox::service_process::exited(pid) {
            return Ok(observed);
        }
    }
    let mut state = observed["state"].clone();
    let _reconciliation = store::lock(&root)?;
    if let Err(error) = super::journal::fence(service, &root, id)
        .and_then(|()| super::journal::recover(service, &root, &mut state))
    {
        state["reconciliation"] = json!({"status":"missing-evidence","reason":error.to_string()});
        return publish(&root, id, state);
    }
    let mut vm_released = false;
    let targets = state["steps"]
        .as_object()
        .map(|s| s.values().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let mut targets = targets;
    if !state["session"].is_null() {
        targets.push(state["session"].clone());
    }
    if root.join("cancel.json").exists() {
        for target in &targets {
            // A settled primary receipt needs no further cancellation request.
            if target == &state["session"] {
                let path = service
                    .join("jobs")
                    .join(target.as_str().ok_or("original job")?)
                    .join("completion.json");
                if path.exists()
                    && completion(&path)
                        .is_some_and(|receipt| receipt["result"]["termination_confirmed"] == true)
                {
                    continue;
                }
            }
            if crate::daemon::request(service, &json!({"op":"task","id":target}))?["state"]
                == "settled"
            {
                continue;
            }
            let command = json!({"op":"cancel","id":target});
            let preview = crate::daemon::request(service, &command)?;
            crate::daemon::request(
                service,
                &json!({"op":"cancel","id":target,"approve":preview["approval_sha256"]}),
            )?;
        }
    }
    if let Some(session) = state["session"].as_str() {
        let main = service.join("jobs").join(session).join("completion.json");
        if !main.exists() {
            state["reconciliation"] =
                json!({"status":"running","reason":"original primary has no completion receipt"});
            return publish(&root, id, state);
        }
        let Some(receipt) = completion(&main) else {
            state["reconciliation"] = json!({"status":"missing-evidence","reason":"original primary receipt incomplete or unavailable"});
            return publish(&root, id, state);
        };
        if receipt["result"]["termination_confirmed"] != true {
            state["reconciliation"] = json!({"status":"missing-evidence","reason":"original primary termination unconfirmed"});
            return publish(&root, id, state);
        }
        vm_released = receipt["result"]["cocoon"]["termination_confirmed"] == true;
    }
    if let Some(steps) = state["steps"].as_object().cloned() {
        for (stage, job) in steps {
            let task = crate::daemon::request(service, &json!({"op":"task","id":job}))?;
            if task["state"] != "settled" {
                state["reconciliation"] =
                    json!({"status":"running","reason":"original validator has not settled"});
                return publish(&root, id, state);
            }
            let instance = Path::new(
                state["instances"][stage]
                    .as_str()
                    .ok_or("original instance")?,
            );
            let path = instance.join("runs/deliver/supervisor/completion.json");
            let Some(completion) = completion(&path) else {
                state["reconciliation"] = json!({"status":"missing-evidence","reason":"original validator receipt incomplete or unavailable"});
                return publish(&root, id, state);
            };
            if completion["result"]["termination_confirmed"] != true
                && completion["result"]["workspace"]["termination_confirmed"] != true
            {
                state["reconciliation"] = json!({"status":"missing-evidence","reason":"original validator termination unconfirmed"});
                return publish(&root, id, state);
            }
        }
    }
    if !vm_released && !super::setup::release(&root)? {
        state["reconciliation"] = json!({"status":"missing-evidence","reason":"original VM release or setup authority unavailable"});
        return publish(&root, id, state);
    }
    if state["runtime_cleanup_required"] == true || root.join("runtime-launch.json").exists() {
        match super::runtime::cleanup(&root) {
            Ok(true) => state["runtime_termination_confirmed"] = json!(true),
            result => {
                let reason = result
                    .err()
                    .map(|error| error.to_string())
                    .unwrap_or_else(|| "original control service termination unconfirmed".into());
                state["reconciliation"] = json!({"status":"missing-evidence","reason":reason});
                return publish(&root, id, state);
            }
        }
    }
    // An interrupted coordinator cannot claim successful delivery from proposals.
    state["state"] = json!(if root.join("cancel.json").exists() {
        "cancelled"
    } else {
        "failed"
    });
    state["termination_confirmed"] = json!(true);
    state["reconciled"] = json!(true);
    state["error"] = json!("original owners settled; interrupted delivery was not replayed");
    state["reconciliation"] = json!({"status":"settled"});
    publish(&root, id, state)
}
fn completion(path: &Path) -> Option<Value> {
    store::read(path, 131072)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
}
fn publish(root: &Path, id: &str, state: Value) -> Result<Value> {
    let pending = root.join(format!(
        ".reconciled-{}.json",
        an_agent_core::det_seam::Entropy::os().uuid_v4()
    ));
    store::write_new(&pending, &serde_json::to_vec(&state)?)?;
    fs::rename(pending, root.join("state.json"))?;
    fs::File::open(root)?.sync_all()?;
    Ok(json!({"id":id,"state":state}))
}
