//! One immutable successor record retires old admission; historical work never moves.
use super::*;

fn route_records(space: &Space) -> Result<Vec<an_agent_core::workspace::WorkspaceRecord>> {
    let records: Vec<_> = space
        .facts
        .log()?
        .into_iter()
        .filter(|r| {
            r.kind == "config"
                && r.name
                    .as_deref()
                    .is_some_and(|n| n.starts_with("native.events."))
        })
        .collect();
    if records.len() > 16 {
        return Err("subscription history limit exceeded".into());
    }
    Ok(records)
}
fn routes(space: &Space) -> Result<Vec<Value>> {
    route_records(space)?
        .iter()
        .map(|r| {
            space.raw_route(
                r.name
                    .as_deref()
                    .and_then(|n| n.strip_prefix("native.events."))
                    .ok_or("route name")?,
            )
        })
        .collect()
}
fn boundary(space: &Space, cutover: &Value) -> Result<usize> {
    let count = cutover["records"].as_u64().ok_or("cutover record count")?;
    let records = space.facts.log()?;
    if count > 512
        || count > records.len() as u64
        || cutover["prefix_sha256"]
            != store::digest(&serde_json::to_vec(&records[..count as usize])?)
    {
        return Err("route cutover history is missing or changed".into());
    }
    Ok(count as usize)
}
pub(super) fn validate(space: &Space, route: &Value) -> Result<()> {
    if route["schema"] == "an-native-event-subscription-v1" {
        return Ok(());
    }
    if space.version < 3 {
        return Err("successor routes require workspace v3".into());
    }
    let count = boundary(space, &route["cutover"])?;
    let records = space.facts.log()?;
    let route_name = format!("native.events.{}", text(route, "name")?);
    let position = records
        .iter()
        .position(|r| r.kind == "config" && r.name.as_deref() == Some(route_name.as_str()))
        .ok_or("successor route is not registered")?;
    let previous_name = text(&route["replaces"], "name")?;
    native::bundle::identifier(previous_name)?;
    let previous_position = records
        .iter()
        .position(|r| {
            r.kind == "config"
                && r.name.as_deref() == Some(&format!("native.events.{previous_name}"))
        })
        .ok_or("missing predecessor route")?;
    let previous = space.raw_route(previous_name)?;
    let parent = hash(&previous)?;
    let successors = routes(space)?
        .iter()
        .filter(|r| {
            r["schema"] == "an-native-event-subscription-v2"
                && r["replaces"]["subscription"] == parent
        })
        .count();
    if successors != 1 {
        return Err("predecessor has ambiguous successors".into());
    }
    if position != count
        || previous_position >= count
        || route["replaces"]["subscription"] != hash(&previous)?
        || route["source"] != previous["source"]
        || route["output"] != previous["output"]
        || route["kind"] != previous["kind"]
    {
        return Err("successor route does not bind its predecessor and cutover".into());
    }
    Ok(())
}
pub(super) fn retired_by(space: &Space, route: &Value) -> Result<Option<Value>> {
    let digest = hash(route)?;
    let mut found = None;
    for candidate in routes(space)? {
        if candidate["schema"] == "an-native-event-subscription-v2"
            && candidate["replaces"]["subscription"] == digest
        {
            validate(space, &candidate)?;
            if found.is_some() || candidate["replaces"]["name"] != route["name"] {
                return Err("ambiguous route replacement".into());
            }
            found = Some(json!({"name":candidate["name"],"subscription":hash(&candidate)?}));
        }
    }
    Ok(found)
}
pub(super) fn not_retired(space: &Space, route: &Value) -> Result<()> {
    if retired_by(space, route)?.is_some() {
        return Err("subscription admission is retired by its approved successor".into());
    }
    Ok(())
}
pub(super) fn eligible(space: &Space, event: &Value, route: &Value) -> Result<()> {
    if route["schema"] != "an-native-event-subscription-v2" {
        return Ok(());
    }
    let count = boundary(space, &route["cutover"])?;
    for record in space.facts.log()?.iter().take(count) {
        if record.kind == "event"
            && record.name.as_deref() == Some(native::event_contract::ARTIFACT)
        {
            let prior: Value = serde_json::from_str(record.body.as_deref().ok_or("event body")?)?;
            if prior["id"] == event["id"] {
                return Err("successor route cannot replay events from before its cutover".into());
            }
        }
    }
    Ok(())
}

pub fn replace(
    path: &Path,
    previous: &str,
    next: &str,
    request: &Path,
    approved: Option<&str>,
) -> Result<Value> {
    if previous == next {
        return Err("replacement requires a fresh subscription name".into());
    }
    // Reuse strict native route request validation; preview never publishes a route.
    let preview = subscription::subscribe(path, next, request, None)?;
    let mut successor = preview["plan"]
        .get("route")
        .cloned()
        .ok_or("successor name already used")?;
    let space = Space::open(path)?;
    let _lock = store::lock(&space.root)?;
    let before = space.route(previous)?;
    space.not_revoked(&before)?;
    not_retired(&space, &before)?;
    space.check_target(&successor)?;
    if identity(Path::new(text(&successor["source"], "root")?))? != successor["source"] {
        return Err("successor producer identity changed".into());
    }
    if successor["source"] != before["source"]
        || successor["output"] != before["output"]
        || successor["kind"] != before["kind"]
    {
        return Err("receiver replacement must preserve the producer and event slot".into());
    }
    if successor["target"] == before["target"]
        && successor["target_generation"] == before["target_generation"]
        && successor["result"] == before["result"]
    {
        return Err("replacement must change the receiver binding or result slot".into());
    }
    let records = space.facts.log()?;
    let name = format!("native.events.{next}");
    if records
        .iter()
        .any(|r| r.kind == "config" && r.name.as_deref() == Some(&name))
    {
        return Err("successor name already registered".into());
    }
    successor["schema"] = json!("an-native-event-subscription-v2");
    successor["replaces"] = json!({"name":previous,"subscription":hash(&before)?});
    successor["cutover"] = json!({"records":records.len(),"prefix_sha256":store::digest(&serde_json::to_vec(&records)?)});
    let plan = json!({"operation":"replace-event-receiver","workspace":space.id,"workspace_schema":"an-native-event-workspace-v3",
        "previous":before,"previous_control":control::current(&space,&before)?,"successor":successor,
        "execution_granted":false,"running_execution_cancelled":false,"historical_events_replayed":false});
    let (digest, execute) = native::approval(&plan, approved)?;
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":digest,"executed":false}));
    }
    if records.len() >= 512 || route_records(&space)?.len() >= 16 {
        return Err("workspace or subscription history is full".into());
    }
    space.upgrade(3)?;
    let bytes = serde_json::to_vec(&successor)?;
    let snapshot = space.root.join("facts/config").join(store::digest(&bytes));
    if snapshot.exists() {
        if regular(&snapshot, 65536)? != bytes {
            return Err("successor snapshot conflicts".into());
        }
    } else {
        store::write_new(&snapshot, &bytes)?;
    }
    fs::File::open(space.root.join("facts/config"))?.sync_all()?;
    space.facts.put_config(&name, &bytes)?;
    fs::File::open(space.root.join("facts/log.jsonl"))?.sync_data()?;
    Ok(
        json!({"status":"succeeded","executed":true,"previous_subscription":hash(&before)?,
        "successor_subscription":hash(&successor)?,"successor":next,"execution_granted":false,
        "running_execution_cancelled":false,"historical_events_replayed":false}),
    )
}

#[cfg(test)]
#[path = "replacement_tests.rs"]
mod tests;
