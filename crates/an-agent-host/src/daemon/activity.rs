//! Bounded activity views over retained original records, never a second ledger.
use super::*;

pub(super) fn view(root: &Path, id: &str) -> Result<Value> {
    let (dir, job) = storage::job(root, id)?;
    let mut events = Vec::new();
    for (name, event) in [
        ("request.json", "submitted"),
        ("claimed.json", "claimed"),
        ("owner.json", "owner-recorded"),
        ("cancel.json", "cancellation-requested"),
        ("launch-error.json", "launch-error-recorded"),
        ("completion.json", "completion-recorded"),
        ("decision.json", "proposal-recorded"),
        ("adoption.json", "operator-adoption-recorded"),
    ] {
        let path = dir.join(name);
        if path.try_exists()? {
            let bytes = store::read(&path, 2 * 1024 * 1024)?;
            events.push(
                json!({"event":event,"source":name,"source_sha256":store::digest(&bytes),
                "evidence":"host-retained-record","artifact_accepted":false}),
            );
        }
    }
    let mut harness = json!({"available":false,"visibility":"unknown"});
    let path = dir.join("native-events.jsonl");
    if matches!(job, Job::Harness { .. }) && path.try_exists()? {
        let bytes = store::read(&path, 2 * 1024 * 1024)?;
        let mut items = Vec::new();
        let mut omitted = 0;
        let mut malformed = 0;
        for (line, raw) in bytes.split(|b| *b == b'\n').enumerate() {
            if raw.is_empty() {
                continue;
            }
            let Ok(value) = serde_json::from_slice::<Value>(raw) else {
                malformed += 1;
                continue;
            };
            if items.len() >= 128 {
                omitted += 1;
                continue;
            }
            // Fixed categories only: raw arguments, messages, URLs and tool output
            // can contain credentials or private source content.
            let kind = match value["type"].as_str().unwrap_or("") {
                "thread.started" | "thread/started" => "session-started",
                "turn.started" | "turn/started" => "turn-started",
                "turn.completed" | "turn/completed" => "turn-completed",
                "turn.failed" => "turn-failed",
                "item.started" | "item/started" => "item-started",
                "item.completed" | "item/completed" => "item-completed",
                _ => "other",
            };
            let action = match value["item"]["type"].as_str().unwrap_or("") {
                "mcp_tool_call" => "tool-call",
                "command_execution" => "command",
                "file_change" => "file-change-report",
                "agent_message" => "message",
                _ => "other",
            };
            items.push(json!({"line":line+1,"event":kind,"action":action}));
        }
        harness = json!({"available":true,"visibility":"reported-events-only",
            "source":"native-events.jsonl","source_sha256":store::digest(&bytes),
            "evidence":"external-harness-report","events":items,"omitted":omitted,"malformed":malformed,
            "grants_authority":false});
    }
    let mut result = json!({"schema":"an-agent-activity-v1","task":observation::task(root,id)?,
        "records":events,"harness":harness,"ordering":"record categories; not a reconstructed timeline"});
    if let Job::Spool { instance, run, .. } = job {
        result["custody"] = spool::status(&instance, &run)?;
    }
    if dir.join("adoption.json").try_exists()? {
        result["adoption"] = read(&dir.join("adoption.json"))?;
    }
    Ok(result)
}

pub(super) fn adopt(root: &Path, req: &Value, supplied: Option<&str>) -> Result<Value> {
    let id = text(req, "id")?;
    let (dir, job) = storage::job(root, id)?;
    if !matches!(job, Job::Harness { .. })
        || !scheduler::result(&job, &dir).is_ok_and(|v| v["proposal_available"] == true)
    {
        return Err("adoption requires a confirmed completed harness proposal".into());
    }
    let instance = Path::new(text(req, "instance")?);
    storage::instance(root, instance)?;
    let custody = spool::artifacts(instance, text(req, "run")?)?;
    let proposal = store::read(&dir.join("decision.json"), 2 * 1024 * 1024)?;
    let (preview, execute) = approval(
        json!({"operation":"record-proposal-adoption",
        "id":id,"request":job.request(),"proposal_sha256":store::digest(&proposal),
        "custody":custody,"assertion":"operator-approved association; no semantic equivalence claim",
        "grants_authority":false}),
        supplied,
    )?;
    if !execute {
        return Ok(preview);
    }
    write(&dir.join("adoption.json"), &preview)?;
    Ok(json!({"recorded":true,"custody_changed":false}))
}
