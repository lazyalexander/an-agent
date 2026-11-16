//! Model working summaries are retained claims, never accepted host facts.
use super::*;

pub(super) fn submit(dir: &Path, snapshot: &Value, req: &Value) -> Result<Value> {
    let allowed = ["op", "proposal", "context_revision", "summary"];
    if req
        .as_object()
        .is_none_or(|r| r.keys().any(|k| !allowed.contains(&k.as_str())))
    {
        return Err("unexpected proposal field".into());
    }
    let summary = text(req, "summary")?;
    let id = text(req, "proposal")?;
    if uuid::Uuid::parse_str(id)?.to_string() != id || summary.len() > 4096 || summary.is_empty() {
        return Err("bounded summary and canonical proposal ID required".into());
    }
    let turn = snapshot["turn"].as_u64().ok_or("turn")?;
    if dir.join(format!("turn-result-{turn}.json")).exists() {
        return Err("proposal requires active original turn".into());
    }
    let directory = dir.join("context-proposals");
    store::directory(&directory)?;
    let path = directory.join(format!("{id}.json"));
    let record = json!({"proposal":id,"context_revision":snapshot["revision"],"execution":snapshot["execution"],
        "turn":turn,"summary":summary,"source":"model-proposal","artifact_accepted":false,"grants_authority":false});
    if path.exists() {
        if read(&path)? != record {
            return Err("proposal identity changed".into());
        }
    } else {
        if storage::entries(dir, "context-proposals", 33)?.len() >= 32 {
            return Err("proposal limit".into());
        }
        // One summary per revision avoids an implicit last-writer-wins policy.
        if !latest(dir, &snapshot["revision"])?.is_null() {
            return Err("revision already has a proposal".into());
        }
        publish(&path, &record)?;
    }
    Ok(record)
}
pub(super) fn latest(dir: &Path, revision: &Value) -> Result<Value> {
    let mut result = Value::Null;
    if !dir.join("context-proposals").exists() {
        return Ok(result);
    }
    for path in storage::entries(dir, "context-proposals", 33)? {
        if path.extension().is_some_and(|e| e == "json") {
            let value = read(&path)?;
            if value["context_revision"] == *revision {
                if !result.is_null() {
                    return Err("ambiguous proposal history".into());
                }
                result = value;
            }
        }
    }
    Ok(result)
}
