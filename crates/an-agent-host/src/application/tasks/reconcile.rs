//! Rebuild only a settled projection from original owner receipts; never resume execution.
use super::*;

pub(in crate::application) fn reconcile(service: &Path, id: &str) -> Result<Value> {
    let observed = view(service, id)?;
    if observed["state"]["state"] != "unknown" {
        return Ok(observed);
    }
    let root = directory(service, id)?;
    let owner = root.join("owner.json");
    if owner.exists() {
        let owner: Value = serde_json::from_slice(&store::read(&owner, 65536)?)?;
        let pid = owner["pid"]
            .as_u64()
            .and_then(|p| u32::try_from(p).ok())
            .ok_or("original owner")?;
        let actual = crate::execution::sandbox::service_process::identity(pid);
        if !actual.is_null() && actual == owner["identity"] {
            return Ok(observed);
        }
    }
    let mut state = observed["state"].clone();
    let Some(session) = state["session"].as_str() else {
        return Ok(observed);
    };
    let main = service.join("jobs").join(session).join("completion.json");
    if !main.exists() {
        return Ok(observed);
    }
    let receipt: Value = serde_json::from_slice(&store::read(&main, 131072)?)?;
    if receipt["result"]["termination_confirmed"] != true
        || receipt["result"]["cocoon"]["termination_confirmed"] != true
    {
        return Ok(observed);
    }
    if let Some(steps) = state["steps"].as_object() {
        for (stage, job) in steps {
            let task = crate::daemon::request(service, &json!({"op":"task","id":job}))?;
            if task["state"] != "settled" {
                return Ok(observed);
            }
            let instance = Path::new(
                state["instances"][stage]
                    .as_str()
                    .ok_or("original instance")?,
            );
            let path = instance.join("runs/deliver/supervisor/completion.json");
            let completion: Value = serde_json::from_slice(&store::read(&path, 131072)?)?;
            if completion["result"]["termination_confirmed"] != true
                && completion["result"]["workspace"]["termination_confirmed"] != true
            {
                return Ok(observed);
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
    let pending = root.join(".reconciled-state.json");
    store::write_new(&pending, &serde_json::to_vec(&state)?)?;
    fs::rename(pending, root.join("state.json"))?;
    Ok(json!({"id":id,"state":state}))
}
