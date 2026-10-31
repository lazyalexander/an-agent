//! Reversible admission control; neither execution cancellation nor route replacement.
use super::*;

const PREFIX: &str = "native.subscription-control.";
const V2: &str = "an-native-event-workspace-v2";

pub(super) fn current(space: &Space, route: &Value) -> Result<Value> {
    let id = hash(route)?;
    let prefix = format!("{PREFIX}{id}.");
    let mut current = json!({"schema":"an-native-subscription-control-v1","workspace":space.id,
        "subscription":id,"generation":0,"mode":"active","previous":Value::Null});
    for record in space.facts.log()? {
        if record.kind != "config"
            || !record
                .name
                .as_deref()
                .is_some_and(|n| n.starts_with(&prefix))
        {
            continue;
        }
        let next = current["generation"].as_u64().ok_or("control generation")? + 1;
        if space.version < 2
            || next > 32
            || record.name.as_deref() != Some(&format!("{prefix}{next}"))
        {
            return Err("subscription control history is out of order or unsupported".into());
        }
        let digest = record.sha256.as_deref().ok_or("control snapshot digest")?;
        if !store::valid_digest(digest) {
            return Err("invalid control snapshot digest".into());
        }
        let bytes = regular(&space.root.join("facts/config").join(digest), 65536)?;
        let stored: Value = serde_json::from_slice(&bytes)?;
        let plan = &stored["plan"];
        let expected = successor(&current)?;
        if store::digest(&bytes) != digest
            || stored["schema"] != "an-native-subscription-control-record-v1"
            || stored["approval_sha256"] != hash(plan)?
            || *plan != transition(&current, &expected)
        {
            return Err("subscription control evidence changed".into());
        }
        current = expected;
    }
    Ok(current)
}
fn successor(before: &Value) -> Result<Value> {
    let mut after = before.clone();
    after["generation"] = json!(before["generation"].as_u64().ok_or("generation")? + 1);
    after["mode"] = json!(if before["mode"] == "active" {
        "paused"
    } else {
        "active"
    });
    after["previous"] = json!(hash(before)?);
    Ok(after)
}
fn transition(before: &Value, after: &Value) -> Value {
    json!({"operation":"subscription-control","workspace_schema":V2,"before":before,"after":after,
        "execution_granted":false,"running_execution_cancelled":false})
}
fn upgrade(space: &Space) -> Result<()> {
    space.upgrade(2)
}

pub fn subscription_control(
    path: &Path,
    name: &str,
    paused: bool,
    approved: Option<&str>,
) -> Result<Value> {
    let space = Space::open(path)?;
    let _lock = store::lock(&space.root)?;
    let route = space.route(name)?;
    space.not_revoked(&route)?;
    replacement::not_retired(&space, &route)?;
    let before = current(&space, &route)?;
    let requested = if paused { "paused" } else { "active" };
    if before["mode"] == requested {
        return Err("subscription already has the requested mode; inspect current control".into());
    }
    if !paused {
        space.check_target(&route)?;
    }
    let after = successor(&before)?;
    let plan = transition(&before, &after);
    let (digest, execute) = native::approval(&plan, approved)?;
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":digest,"executed":false}));
    }
    let generation = after["generation"].as_u64().ok_or("generation")?;
    if generation > 32 || space.facts.log()?.len() >= 512 {
        return Err("subscription control or workspace record limit reached".into());
    }
    upgrade(&space)?;
    let stored = json!({"schema":"an-native-subscription-control-record-v1","plan":plan,"approval_sha256":digest});
    let bytes = serde_json::to_vec(&stored)?;
    let snapshot = space.root.join("facts/config").join(store::digest(&bytes));
    if snapshot.exists() {
        if regular(&snapshot, 65536)? != bytes {
            return Err("control snapshot conflicts".into());
        }
    } else {
        store::write_new(&snapshot, &bytes)?;
    }
    fs::File::open(space.root.join("facts/config"))?.sync_all()?;
    space
        .facts
        .put_config(&format!("{PREFIX}{}.{}", hash(&route)?, generation), &bytes)?;
    fs::File::open(space.root.join("facts/log.jsonl"))?.sync_data()?;
    Ok(json!({"status":"succeeded","executed":true,"control":after,
        "execution_granted":false,"running_execution_cancelled":false}))
}

#[cfg(test)]
#[path = "control_tests.rs"]
mod tests;
