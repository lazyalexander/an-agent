//! Operator-approved additional input bytes; original session authority still gates use.
use super::*;
pub(crate) fn attach(root: &Path, req: &Value, approved: Option<&str>) -> Result<Value> {
    let (dir, job) = storage::job(root, text(req, "id")?)?;
    let Job::Harness {
        authorization: Some(authority),
        task,
        ..
    } = job
    else {
        return Err("bound session required".into());
    };
    crate::task_permissions::active(&authority)?;
    if dir.join("cancel.json").exists() || dir.join("completion.json").exists() {
        return Err("session closed".into());
    }
    let binding = crate::task_permissions::binding(root, text(req, "input_grant")?)?;
    crate::task_permissions::active(&binding)?;
    let grant = crate::task_permissions::load(&binding)?;
    let original = crate::task_permissions::load(&authority)?;
    if grant.scope.task != original.scope.task
        || grant.scope.delegate
        || grant.scope.expires_ms > original.scope.expires_ms
        || grant.scope.permissions.len() != 1
        || !matches!(
            grant.scope.permissions[0],
            crate::task_permissions::Permission::ReadInput { .. }
        )
    {
        return Err("attachment must be one bounded input for the same task".into());
    }
    let mut plan = json!({"operation":"attach-session-input","id":text(req,"id")?,"session_authority":authority,"input_grant":binding});
    if let Some(name) = req.get("spool_binding") {
        let mut selection = json!({"op":"read-input","spool_binding":name});
        let target = context::authority(&task, &authority, &mut selection)?;
        if target == authority {
            return Err("attachment spool binding required".into());
        }
        plan["target_authority"] = json!(target);
    }
    let (preview, execute) = approval(plan, approved)?;
    if execute {
        let directory = dir.join("input-grants");
        store::directory(&directory)?;
        if storage::entries(&dir, "input-grants", 8)?.len() >= 8 {
            return Err("session input attachment limit".into());
        }
        let path = directory.join(format!("{}.json", binding.id));
        if path.exists() {
            if read(&path)? != preview["plan"] {
                return Err("input attachment changed".into());
            }
        } else {
            publish(&path, &preview["plan"])?;
        }
    }
    Ok(preview)
}
pub(crate) fn input(
    root: &Path,
    session: &Path,
    req: &Value,
    authority: &Binding,
) -> Result<(Value, Binding)> {
    crate::task_permissions::active(authority)?;
    if let Ok(value) = permissions::dispatch_bound(root, req, authority) {
        return Ok((value, authority.clone()));
    }
    if req["op"] != "read-input" {
        return Err("input reference not authorized".into());
    }
    let directory = session.join("input-grants");
    if directory.exists() {
        for path in storage::entries(session, "input-grants", 8)? {
            let record = read(&path)?;
            if record["session_authority"] != json!(authority)
                && record["target_authority"] != json!(authority)
            {
                continue;
            }
            let original: Binding = serde_json::from_value(record["session_authority"].clone())?;
            crate::task_permissions::active(&original)?;
            let binding: Binding = serde_json::from_value(record["input_grant"].clone())?;
            if let Ok(value) = permissions::dispatch_bound(root, req, &binding) {
                return Ok((value, binding));
            }
        }
    }
    Err("input reference not authorized".into())
}

pub(crate) fn active(session: &Path, authority: &Binding) -> Result<()> {
    crate::task_permissions::active(authority)?;
    if session.join("input-grants").exists() {
        for path in storage::entries(session, "input-grants", 8)? {
            let record = read(&path)?;
            if record["session_authority"] != json!(authority) {
                return Err("input session binding mismatch".into());
            }
            let binding: Binding = serde_json::from_value(record["input_grant"].clone())?;
            crate::task_permissions::active(&binding)?;
        }
    }
    Ok(())
}

/// References only: attachment approval is not proof that bytes were disclosed.
pub(super) fn evidence(session: &Path, authority: &Binding) -> Result<Value> {
    active(session, authority)?;
    let mut inputs = Vec::new();
    if !session.join("input-grants").exists() {
        return Ok(json!(inputs));
    }
    for path in storage::entries(session, "input-grants", 8)? {
        let record = read(&path)?;
        let binding: Binding = serde_json::from_value(record["input_grant"].clone())?;
        let grant = crate::task_permissions::load(&binding)?;
        inputs.push(
            json!({"grant_ref":{"id":binding.id,"sha256":binding.sha256},
            "permissions":grant.scope.permissions,"target_grant":record["target_authority"]["id"],
            "disclosure":"requires-receipt"}),
        );
    }
    Ok(json!(inputs))
}
