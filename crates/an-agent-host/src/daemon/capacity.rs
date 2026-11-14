//! Explicit idle-service capacity preparation for a retained session plus validator.
use super::*;
pub(super) fn prepare(root: &Path, req: &Value) -> Result<Value> {
    let command = text(req, "command")?;
    if uuid::Uuid::parse_str(command)?.to_string() != command {
        return Err("capacity command identity required".into());
    }
    let records = root.join("capacity-changes");
    store::directory(&records)?;
    let record = records.join(format!("{command}.json"));
    if record.exists() {
        let plan = read(&record)?;
        let hash = store::digest(&serde_json::to_vec(&plan)?);
        if req
            .get("approve")
            .is_some_and(|value| value != &json!(hash))
        {
            return Err("capacity approval changed".into());
        }
        return Ok(json!({"plan":plan,"approval_sha256":hash,"executed":true,"replayed":false}));
    }
    let bytes = store::read(&root.join("config.json"), 65536)?;
    let config = storage::config(root)?;
    if config.concurrency >= 2 {
        return Ok(json!({"ready":true,"changed":false}));
    }
    let snapshot = observation::snapshot(root)?;
    if snapshot["tasks"]
        .as_array()
        .is_some_and(|tasks| tasks.iter().any(|task| task["state"] != "settled"))
    {
        return Err("capacity changes require a settled idle service".into());
    }
    let plan = json!({"operation":"prepare-document-capacity","command":command,"from":config.concurrency,"to":2,"configuration_sha256":store::digest(&bytes)});
    let (preview, execute) = approval(plan, req.get("approve").and_then(Value::as_str))?;
    if !execute {
        return Ok(preview);
    }
    let mut updated: Value = serde_json::from_slice(&bytes)?;
    updated["concurrency"] = json!(2);
    let temporary = root.join(format!(".config-{command}.pending"));
    write(&temporary, &updated)?;
    // Both old and new settings remain valid if publication is interrupted.
    fs::rename(&temporary, root.join("config.json"))?;
    fs::File::open(root)?.sync_all()?;
    write(&record, &preview["plan"])?;
    Ok(json!({"ready":true,"changed":true,"replayed":false}))
}
