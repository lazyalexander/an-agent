//! Provider-neutral execution, capabilities and reconnect over original task custody.
use super::*;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Kind {
    CodexExec,
    CodexAppServer,
}

pub(super) trait HarnessAdapter {
    fn capabilities(&self) -> Value;
    fn execute(
        &self,
        root: &Path,
        dir: &Path,
        task: &harness::Task,
        authority: Option<&Binding>,
        cancelled: &AtomicBool,
    ) -> Result<Value>;
    fn event(&self, native: &Value) -> Value;
}

pub(super) fn kind(task: &harness::Task) -> Result<Kind> {
    if task.app_server && task.adapter == Some(Kind::CodexExec) {
        return Err("conflicting harness adapter selectors".into());
    }
    Ok(task.adapter.unwrap_or(if task.app_server {
        Kind::CodexAppServer
    } else {
        Kind::CodexExec
    }))
}
pub(super) fn interactive(task: &harness::Task) -> Result<bool> {
    Ok(kind(task)? == Kind::CodexAppServer)
}
pub(super) fn selected(task: &harness::Task) -> Result<&'static dyn HarnessAdapter> {
    Ok(match kind(task)? {
        Kind::CodexExec => &super::codex::Codex { interactive: false },
        Kind::CodexAppServer => &super::codex::Codex { interactive: true },
    })
}

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
        let capability = if op == "harness-continue" {
            "continue_turn"
        } else {
            "steer"
        };
        if adapter.capabilities()[capability] != true {
            return Err("adapter does not support the requested control".into());
        }
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
