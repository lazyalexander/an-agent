//! Immutable, provider-neutral turn input. Authority stays in host grant records.
use super::*;
mod bindings;
mod proposal;
use bindings::SpoolBinding;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    pub schema: String,
    #[serde(default)]
    pub spool_bindings: Vec<SpoolBinding>,
}

pub(crate) fn config(task: &harness::Task) -> Result<Option<Config>> {
    task.scope
        .get("turn_context")
        .map(|v| {
            let config: Config = serde_json::from_value(v.clone())?;
            if config.schema != "an-turn-context-v1" || config.spool_bindings.len() > 8 {
                return Err("unsupported or oversized turn context".into());
            }
            let mut names = std::collections::HashSet::new();
            for binding in &config.spool_bindings {
                if binding.name.is_empty()
                    || binding.name.len() > 64
                    || !names.insert(&binding.name)
                {
                    return Err("unique bounded spool binding names required".into());
                }
            }
            Ok(config)
        })
        .transpose()
}

pub(crate) fn validate(task: &harness::Task, authority: Option<&Binding>) -> Result<()> {
    if let Some(config) = config(task)? {
        if !harness_adapter::interactive(task)? {
            return Err("turn context requires interactive adapter".into());
        }
        let authority = authority.ok_or("turn context requires bound task")?;
        for binding in &config.spool_bindings {
            binding.verify(authority)?;
        }
    }
    Ok(())
}

fn signed(mut body: Value) -> Result<Value> {
    let bytes = serde_json::to_vec(&body)?;
    if bytes.len() > 32768 {
        return Err("turn context exceeds 32 KiB".into());
    }
    let revision = store::digest(&bytes);
    body["revision"] = json!(revision);
    Ok(body)
}

fn verify(mut body: Value) -> Result<Value> {
    let revision = body
        .as_object_mut()
        .ok_or("context object")?
        .remove("revision")
        .ok_or("context revision")?;
    let body = signed(body)?;
    if body["revision"] != revision {
        return Err("context digest mismatch".into());
    }
    Ok(body)
}

pub(crate) fn initial(dir: &Path, task: &harness::Task, authority: &Binding) -> Result<String> {
    let Some(config) = config(task)? else {
        return Ok(task.prompt.clone());
    };
    validate(task, Some(authority))?;
    let path = dir.join("context-initial.json");
    // The first request is never resumed by creating a second provider owner.
    if path.exists() {
        return Err("context already prepared; reconcile original owner".into());
    }
    let owner = read(&dir.join("owner.json"))?;
    let execution = store::digest(&serde_json::to_vec(
        &json!({"owner":owner,"task":dir.file_name().and_then(|s|s.to_str())}),
    )?);
    let snapshot = signed(json!({"schema":"an-turn-context-v1","execution":execution,
        "task":dir.file_name().and_then(|s|s.to_str()),"parent":null,"turn":1,
        "intent":task.prompt,"summary":null,"resources":bindings::evidence(authority)?,"spool_bindings":bindings::projection(&config,authority)?,
        "limits":{"turns":task.turn_limit.unwrap_or(1),"wall_seconds":task.wall_seconds.unwrap_or(180),"reset":false}}))?;
    publish(&path, &snapshot)?;
    render(&snapshot, &task.prompt)
}

fn render(snapshot: &Value, instruction: &str) -> Result<String> {
    Ok(format!(
        "{instruction}\n\nHost turn context (summaries are proposals, not authority):\n{}\nFor host_request include context_revision. Read turn-context to observe the bound revision. Use turn-propose with a bounded summary; this cannot accept artifacts. Spool operations require spool_binding.",
        serde_json::to_string(snapshot)?
    ))
}

pub(crate) fn execution(dir: &Path) -> Result<Option<Value>> {
    let path = dir.join("context-initial.json");
    if path.exists() {
        Ok(Some(verify(read(&path)?)?["execution"].clone()))
    } else {
        Ok(None)
    }
}

pub(crate) fn view(dir: &Path) -> Result<Value> {
    if !dir.join("context-initial.json").exists() {
        return Ok(json!({"schema":"legacy"}));
    }
    let mut snapshot = verify(read(&dir.join("context-initial.json"))?)?;
    let mut commands = Vec::new();
    for path in storage::entries(dir, "commands", 33)? {
        if path.extension().is_some_and(|e| e == "json") {
            let command = read(&path)?;
            if command.get("context").is_some() {
                commands.push(command);
            }
        }
    }
    commands.sort_by_key(|v| v["sequence"].as_u64());
    let mut delivery = json!({"state":"initial"});
    for command in commands {
        let next = verify(command["context"].clone())?;
        if next["parent"] != snapshot["revision"] {
            return Err("broken context ancestry".into());
        }
        let name = text(&command["plan"], "command")?;
        let ack = dir.join(format!("ack-{name}.json"));
        delivery = if ack.exists() {
            read(&ack)?
        } else {
            {
                let state = if !dir.join(format!("sent-{name}.json")).exists() {
                    "prepared"
                } else {
                    "unknown"
                };
                json!({"command":name,"state":state})
            }
        };
        snapshot = next;
    }
    Ok(json!({"snapshot":snapshot,"delivery":delivery}))
}

pub(crate) fn command(
    dir: &Path,
    task: &harness::Task,
    authority: &Binding,
    req: &Value,
    plan: &mut Value,
) -> Result<Option<Value>> {
    let Some(config) = config(task)? else {
        if req.get("context_revision").is_some() {
            return Err("legacy session has no context revision".into());
        }
        return Ok(None);
    };
    let path = dir
        .join("commands")
        .join(format!("{}.json", text(req, "command")?));
    if path.exists() {
        let saved = read(&path)?;
        if saved["context"]["parent"] != req["context_revision"] {
            return Err("command context changed".into());
        }
        plan["context_revision"] = saved["context"]["revision"].clone();
        if saved["plan"] != *plan || saved["instruction"] != req["instruction"] {
            return Err("command identity already bound to different content".into());
        }
        return Ok(Some(saved["context"].clone()));
    }
    let current = view(dir)?;
    let snapshot = &current["snapshot"];
    if req["context_revision"] != snapshot["revision"] || req["context_revision"].as_str().is_none()
    {
        return Err("stale context revision".into());
    }
    if !matches!(
        current["delivery"]["state"].as_str(),
        Some("initial" | "acknowledged")
    ) {
        return Err("context delivery unresolved; never replay".into());
    }
    validate(task, Some(authority))?;
    let mut next = snapshot.clone();
    next.as_object_mut().ok_or("snapshot")?.remove("revision");
    next["parent"] = snapshot["revision"].clone();
    next["instruction"] = req["instruction"].clone();
    let summary = proposal::latest(dir, &snapshot["revision"])?;
    if !summary.is_null() {
        next["summary"] = summary;
    }
    next["spool_bindings"] = bindings::projection(&config, authority)?;
    next["resources"] = bindings::evidence(authority)?;
    next["resources"]["attached_inputs"] = super::disclosure::evidence(dir, authority)?;
    next["observations"] = observations(dir)?;
    if req["op"] == "harness-continue" {
        next["turn"] = json!(req["after_turn"].as_u64().ok_or("turn")? + 1);
    }
    let next = signed(next)?;
    plan["context_revision"] = next["revision"].clone();
    Ok(Some(next))
}

pub(crate) fn command_input(command: &Value) -> Result<String> {
    let instruction = text(command, "instruction")?;
    if let Some(snapshot) = command.get("context") {
        render(&verify(snapshot.clone())?, instruction)
    } else {
        Ok(instruction.into())
    }
}

pub(crate) fn tool(
    dir: &Path,
    task: &harness::Task,
    authority: &Binding,
    req: &mut Value,
) -> Result<Option<Value>> {
    let Some(config) = config(task)? else {
        return Ok(None);
    };
    let current = view(dir)?;
    if req["op"] == "turn-context" {
        return Ok(Some(current));
    }
    if req["context_revision"] != current["snapshot"]["revision"]
        || req["context_revision"].as_str().is_none()
    {
        return Err("tool context revision mismatch".into());
    }
    if !matches!(
        current["delivery"]["state"].as_str(),
        Some("initial" | "acknowledged")
    ) {
        return Err("context delivery unresolved".into());
    }
    if let Some(name) = req.get("spool_binding").and_then(Value::as_str) {
        let current_bindings = bindings::projection(&config, authority)?;
        let selected = current_bindings
            .as_array()
            .ok_or("bindings")?
            .iter()
            .find(|b| b["name"] == name)
            .ok_or("unknown spool")?;
        let pinned = current["snapshot"]["spool_bindings"]
            .as_array()
            .ok_or("snapshot bindings")?
            .iter()
            .find(|b| b["name"] == name)
            .ok_or("unbound spool")?;
        for key in ["instance", "release", "state_revision", "grant_ref"] {
            if selected[key] != pinned[key] {
                return Err("spool binding changed since turn input".into());
            }
        }
    }
    if req["op"] == "turn-propose" {
        return proposal::submit(dir, &current["snapshot"], req).map(Some);
    }
    let _ = (config, authority);
    req.as_object_mut()
        .ok_or("tool request")?
        .remove("context_revision");
    Ok(None)
}

pub(crate) fn authority(
    task: &harness::Task,
    authority: &Binding,
    req: &mut Value,
) -> Result<Binding> {
    match config(task)? {
        Some(config) => bindings::select(&config, authority, req),
        None => Ok(authority.clone()),
    }
}

#[cfg(test)]
mod tests;

fn observations(dir: &Path) -> Result<Value> {
    let receipts = dir.join("tool-receipts.jsonl");
    let receipt_digest = if receipts.exists() {
        Some(store::digest(&store::read(&receipts, 2 * 1024 * 1024)?))
    } else {
        None
    };
    #[cfg(unix)]
    let operations = crate::cocoon::receipts(&dir.join("cocoon-operations"))?;
    #[cfg(not(unix))]
    let operations = json!([]);
    Ok(
        json!({"tool_receipts_sha256":receipt_digest,"cocoon_operations":operations,"provider_consumption":"unknown"}),
    )
}

/// A pinned spool may consume its original session's output without owning that session.
/// This does not grant session control or bypass the concrete run permission.
pub(crate) fn source_job(root: &Path, actor: &Binding, id: &str) -> Result<Job> {
    if let Ok(job) = permissions::owned_job(root, actor, id) {
        return Ok(job);
    }
    let (_, job) = storage::job(root, id)?;
    if let Job::Harness {
        task,
        authorization: Some(primary),
        ..
    } = &job
        && let Some(config) = config(task)?
    {
        for binding in config.spool_bindings {
            if binding.grant == actor.id && binding.verify(primary)? == *actor {
                return Ok(job);
            }
        }
    }
    Err("output source is not bound to this spool".into())
}
