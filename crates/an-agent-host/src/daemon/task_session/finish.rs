//! Explicit normal closure of an idle original session, distinct from cancellation.
use super::*;

pub(super) fn admit(
    dir: &Path,
    job: &Job,
    adapter: &dyn super::super::harness_adapter::HarnessAdapter,
    req: &Value,
    approved: Option<&str>,
) -> Result<Value> {
    if !adapter.capabilities().finish_session {
        return Err("adapter cannot finish an idle session".into());
    }
    let Job::Harness { task, .. } = job else {
        return Err("not a harness task".into());
    };
    let session = read(&dir.join("session.json"))?;
    if req["execution"] != session["execution"] {
        return Err("stale execution handle".into());
    }
    let after = req["after_turn"]
        .as_u64()
        .ok_or("completed turn required")?;
    let plan = json!({"operation":"finish-harness","id":text(req,"id")?,"request":job.request(),
        "execution":session["execution"],"after_turn":after});
    let (preview, execute) = approval(plan, approved)?;
    let marker = dir.join("finish.json");
    if marker.exists() {
        if read(&marker)? != preview["plan"] {
            return Err("session already has a different finish binding".into());
        }
        return Ok(if execute {
            json!({"finishing":true,"replayed":false})
        } else {
            preview
        });
    }
    let state = turn_state(dir, task.turn_limit.unwrap_or(1))?;
    if after == 0
        || state["completed"] != after
        || !state["active"].is_null()
        || dir.join("cancel.json").exists()
        || dir.join("completion.json").exists()
        || !owner_live(dir)?
    {
        return Err("finish requires its idle live original session".into());
    }
    for path in storage::entries(dir, "commands", 33)? {
        if path.extension().is_some_and(|ext| ext == "json") {
            let command = read(&path)?;
            if command["plan"]["after_turn"] == after {
                return Err("continuation already admitted for this turn".into());
            }
        }
    }
    if !execute {
        return Ok(preview);
    }
    publish(&marker, &preview["plan"])?;
    Ok(json!({"finishing":true,"replayed":false}))
}

pub(crate) fn ready(dir: &Path, completed: u8) -> Result<bool> {
    let path = dir.join("finish.json");
    if !path.exists() {
        return Ok(false);
    }
    let value = read(&path)?;
    let session = read(&dir.join("session.json"))?;
    if value["execution"] != session["execution"] || value["after_turn"] != completed {
        return Err("finish binding does not match the original idle session".into());
    }
    Ok(true)
}
