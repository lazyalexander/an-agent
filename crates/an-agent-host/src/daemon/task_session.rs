//! Host-owned session controls and authorized tools; provider protocols stay in adapters.
use super::harness_adapter::{kind, selected};
use super::*;
use std::io::Write;

pub(super) fn dispatch(root: &Path, req: &Value, approved: Option<&str>) -> Result<Value> {
    let id = text(req, "id")?;
    let (dir, job) = storage::job(root, id)?;
    let Job::Harness {
        task,
        authorization,
        ..
    } = &job
    else {
        return Err("not a harness task".into());
    };
    let adapter = selected(task)?;
    let op = text(req, "op")?;
    if op == "harness-turn-result" {
        let number = req["turn"].as_u64().ok_or("turn number required")?;
        if number == 0 || number > u64::from(task.turn_limit.unwrap_or(1)) {
            return Err("turn outside task limit".into());
        }
        return read(&dir.join(format!("turn-result-{number}.json")));
    }
    if matches!(op, "harness-steer" | "harness-continue") {
        require_control(adapter, op)?;
        let authority = authorization.as_ref().ok_or("missing task authority")?;
        crate::task_permissions::active(authority)?;
        let instruction = text(req, "instruction")?;
        let command = text(req, "command")?;
        if instruction.is_empty()
            || instruction.len() > 65536
            || command.is_empty()
            || command.len() > 64
            || !command
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        {
            return Err("bounded instruction and command identity required".into());
        }
        let session = read(&dir.join("session.json"))?;
        if req["execution"] != session["execution"] {
            return Err("stale execution handle".into());
        }
        let commands = dir.join("commands");
        let path = commands.join(format!("{command}.json"));
        let mut plan = json!({"operation":"steer-harness","id":id,"request":job.request(),
            "execution":session["execution"],"command":command,"instruction_sha256":store::digest(instruction.as_bytes())});
        if op == "harness-steer" {
            let turn = if path.exists() {
                read(&path)?["plan"]["turn"].clone()
            } else {
                turn_state(&dir, task.turn_limit.unwrap_or(1))?["active"].clone()
            };
            if turn.is_null() && !path.exists() {
                return Err(
                    "steering requires an active turn; continue a completed turn explicitly".into(),
                );
            }
            if !turn.is_null() {
                plan["turn"] = turn;
            }
        }
        if op == "harness-continue" {
            let after = req["after_turn"]
                .as_u64()
                .ok_or("completed turn required")?;
            if after == 0
                || after >= u64::from(task.turn_limit.unwrap_or(1))
                || !dir.join(format!("turn-result-{after}.json")).exists()
            {
                return Err(
                    "continuation requires a completed turn within the original limit".into(),
                );
            }
            plan["operation"] = json!("continue-harness");
            plan["after_turn"] = json!(after);
        }
        let (preview, execute) = approval(plan, approved)?;
        if !execute {
            return Ok(preview);
        }
        store::directory(&commands)?;
        let mut record = json!({"plan":preview["plan"],"instruction":instruction});
        // Same command identity reconciles the original submission, including after completion.
        if path.exists() {
            let original = read(&path)?;
            if original["plan"] != record["plan"]
                || original["instruction"] != record["instruction"]
            {
                return Err("command identity already bound to different content".into());
            }
            return Ok(json!({"command":command,"queued":true,"replayed":false}));
        }
        if op == "harness-continue" {
            for path in storage::entries(&dir, "commands", 33)? {
                if path.extension().is_some_and(|ext| ext == "json")
                    && read(&path)?["plan"]["after_turn"] == record["plan"]["after_turn"]
                {
                    return Err("completed turn already has a continuation command".into());
                }
            }
        }
        if !owner_live(&dir)? {
            return Err("original owner unavailable; never replay".into());
        }
        let sequence = storage::entries(&dir, "commands", 33)?.len();
        if dir.join("completion.json").exists()
            || dir.join("cancel.json").exists()
            || sequence >= 32
        {
            return Err("task closed or command limit reached".into());
        }
        record["sequence"] = json!(sequence);
        publish(&path, &record)?;
        return Ok(json!({"command":command,"queued":true,"acknowledged":false}));
    }
    let cursor = req
        .get("cursor")
        .map(|v| v.as_u64().ok_or("invalid event cursor"))
        .transpose()?
        .unwrap_or(0);
    let path = dir.join("native-events.jsonl");
    let bytes = if path.exists() {
        store::read(&path, 2 * 1024 * 1024)?
    } else {
        Vec::new()
    };
    let cursor = usize::try_from(cursor)?;
    if cursor > bytes.len() || (cursor > 0 && bytes[cursor - 1] != b'\n') {
        return Err("event cursor must name a retained record boundary".into());
    }
    let mut next = cursor;
    let mut events = Vec::new();
    for raw in bytes[cursor..].split_inclusive(|b| *b == b'\n').take(128) {
        if raw.last() != Some(&b'\n') {
            break;
        }
        let native: Value = serde_json::from_slice(raw)?;
        next += raw.len();
        let mut event = adapter.event(&native);
        event["cursor"] = json!(next);
        events.push(event);
    }
    let live = owner_live(&dir)?;
    let mut commands = Vec::new();
    for path in if dir.join("commands").exists() {
        storage::entries(&dir, "commands", 33)?
    } else {
        Vec::new()
    } {
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|v| v.to_str())
            .ok_or("command ID")?;
        let receipt = dir.join(format!("ack-{name}.json"));
        commands.push(if receipt.exists() { read(&receipt)? } else {
            json!({"command":name,"state":if !live || dir.join("completion.json").exists() {"unknown"} else {"pending"}})
        });
    }
    let execution = if dir.join("session.json").exists() {
        read(&dir.join("session.json"))?["execution"].clone()
    } else {
        Value::Null
    };
    Ok(
        json!({"id":id,"adapter":kind(task)?,"execution":execution,"capabilities":adapter.capabilities(),
        "turns":turn_state(&dir, task.turn_limit.unwrap_or(1))?,
        "task":observation::task(root,id)?,"events":events,"cursor":next,"commands":commands,"automatic_retries":0}),
    )
}

fn turn_state(dir: &Path, limit: u8) -> Result<Value> {
    let mut completed = 0;
    let mut active = Value::Null;
    for number in 1..=limit {
        if dir.join(format!("turn-result-{number}.json")).exists() {
            completed = number;
        } else if dir.join(format!("turn-start-{number}.json")).exists() {
            active = read(&dir.join(format!("turn-start-{number}.json")))?["turn"].clone();
        }
    }
    // Original one-turn owners created before continuation support retain session.json.
    if active.is_null()
        && completed == 0
        && limit == 1
        && dir.join("session.json").exists()
        && !dir.join("completion.json").exists()
    {
        active = read(&dir.join("session.json"))?["turn"].clone();
    }
    Ok(json!({"completed":completed,"limit":limit,"active":active}))
}

/// Cross-process records become visible only after their complete bytes are durable.
pub(super) fn publish(path: &Path, value: &Value) -> Result<()> {
    let staging = path.with_extension("pending");
    write(&staging, value)?;
    fs::hard_link(&staging, path)?;
    fs::remove_file(staging)?;
    fs::File::open(path.parent().ok_or("record parent")?)?.sync_all()?;
    Ok(())
}

fn owner_live(dir: &Path) -> Result<bool> {
    if !dir.join("owner.json").exists() {
        return Ok(false);
    }
    let owner = read(&dir.join("owner.json"))?;
    let pid = owner["pid"]
        .as_u64()
        .and_then(|p| u32::try_from(p).ok())
        .ok_or("owner PID")?;
    let identity = crate::execution::sandbox::service_process::identity(pid);
    Ok(!identity.is_null() && identity == owner["process_identity"])
}

pub(super) fn tool_request(root: &Path, envelope: &Value) -> Result<Value> {
    let (dir, job) = storage::job(root, text(envelope, "id")?)?;
    let Job::Harness {
        task,
        authorization: Some(authority),
        ..
    } = job
    else {
        return Err("no harness authority".into());
    };
    if !super::harness_adapter::interactive(&task)?
        || !dir.join("owner.json").exists()
        || dir.join("cancel.json").exists()
        || dir.join("completion.json").exists()
    {
        return Err("task session is not active".into());
    }
    let started = an_agent_core::det_seam::Clock::monotonic_now();
    let result = host_tool(root, &authority, &envelope["arguments"]);
    let receipt = json!({"request_sha256":store::digest(&serde_json::to_vec(&envelope["arguments"])?),
        "grant":authority.id,"succeeded":result.is_ok(),
        "observed_ms":super::harness_metrics::now_ms(),"elapsed_ms":started.elapsed().as_millis(),
        "elapsed_us":started.elapsed().as_micros(),
        "disclosure":result.as_ref().ok().and_then(|response| super::harness_metrics::disclosure(&envelope["arguments"], response)),
        "response_sha256":store::digest(&serde_json::to_vec(&match &result { Ok(v)=>v.clone(), Err(e)=>json!({"error":e.to_string()}) })?)});
    let path = dir.join("tool-receipts.jsonl");
    store::no_links(&path)?;
    if !path.exists() {
        store::write_new(&path, b"")?;
    }
    let mut file = fs::OpenOptions::new().append(true).open(path)?;
    serde_json::to_writer(&mut file, &receipt)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    result
}

fn host_tool(root: &Path, authority: &Binding, arguments: &Value) -> Result<Value> {
    let mut req: Value = serde_json::from_str(text(arguments, "request")?)?;
    let op = text(&req, "op")?;
    if matches!(
        op,
        "derive-grant" | "grant" | "revoke-grant" | "harness-permission"
    ) {
        return Err("harness cannot issue credentials or administer grants".into());
    }
    let selected = if let Some(id) = req.get("grant").and_then(Value::as_str) {
        let child = crate::task_permissions::binding(root, id)?;
        if !crate::task_permissions::load(authority)?.scope.delegate
            || !crate::task_permissions::owns(authority, &child)?
            || child == *authority
        {
            return Err("delegation requires an authorized descendant grant".into());
        }
        child
    } else {
        authority.clone()
    };
    req.as_object_mut().ok_or("host request")?.remove("grant");
    crate::task_permissions::active(authority)?;
    permissions::dispatch_bound(root, &req, &selected)
}

fn require_control(adapter: &dyn super::harness_adapter::HarnessAdapter, op: &str) -> Result<()> {
    let capabilities = adapter.capabilities();
    let supported = match op {
        "harness-continue" => capabilities.continue_turn,
        "harness-steer" => capabilities.steer,
        _ => false,
    };
    if !supported {
        return Err("adapter does not support the requested control".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
