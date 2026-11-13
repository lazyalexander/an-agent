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
    if text(req, "op")? == "harness-steer" {
        if adapter.capabilities()["steer"] != true {
            return Err("adapter does not support steering".into());
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
        let plan = json!({"operation":"steer-harness","id":id,"request":job.request(),
            "execution":session["execution"],"command":command,"instruction_sha256":store::digest(instruction.as_bytes())});
        let (preview, execute) = approval(plan, approved)?;
        if !execute {
            return Ok(preview);
        }
        let commands = dir.join("commands");
        store::directory(&commands)?;
        let path = commands.join(format!("{command}.json"));
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
        "task":observation::task(root,id)?,"events":events,"cursor":next,"commands":commands,"automatic_retries":0}),
    )
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
