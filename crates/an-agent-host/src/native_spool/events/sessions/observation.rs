//! Observation takes no control locks; closure rechecks original execution evidence.
use super::*;

pub(super) fn settled(space: &Space, record: &Value, path: &Path) -> Result<Value> {
    let intent = read(&path.join("intent.json"))?;
    let plan = &intent["plan"];
    if plan["session"] != *record
        || intent["approval_sha256"] != hash(plan)?
        || path.file_name().and_then(|s| s.to_str()) != plan["thread"].as_str()
        || plan["request_sha256"] != store::digest(&regular(&path.join("request.json"), 65536)?)
    {
        return Err("thread identity mismatch".into());
    }
    let receipt = read(&path.join("result.json"))?;
    if receipt["approval_sha256"] != intent["approval_sha256"] {
        return Err("thread receipt mismatch".into());
    }
    let dispatch_name = text(&plan["dispatch"]["plan"]["request"], "name")?;
    let dir = space.root.join("dispatches").join(dispatch_name);
    if !dir.exists() {
        if receipt["result"]["executed"] == false
            && receipt["result"]["reason"] == "stopped-before-admission"
        {
            return Ok(json!({"thread":plan["thread"],"settled":true,"attempts":[]}));
        }
        return Err("dispatch admission unknown".into());
    }
    if receipt["result"] != read(&dir.join("result.json"))? {
        return Err("thread and original dispatch result differ".into());
    }
    let original = dispatch::recorded(space, &dir)?;
    if original["plan"] != plan["dispatch"]["plan"] {
        return Err("dispatch selection changed".into());
    }
    let observed = dispatch::inspect_dispatch(&space.root, dispatch_name)?;
    if observed["status"] == "uncertain" {
        return Err("dispatch outcome unresolved".into());
    }
    let entries = observed["entries"].as_array().ok_or("dispatch entries")?;
    let selected = original["plan"]["entries"]
        .as_array()
        .ok_or("dispatch selection")?;
    let mut attempts = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let expected = &selected.get(index).ok_or("extra dispatch entry")?["plan"];
        let target = Path::new(text(&expected["route"]["target"], "root")?);
        let run = text(&expected["context"], "delivery")?;
        let result = native::run::settled(target, run)?;
        if result["workspace"]["termination_confirmed"] != true
            || entry["result"]["result"] != result
            || read(&target.join("runs").join(run).join("start.json"))? != expected["start"]
        {
            return Err("thread termination evidence mismatch".into());
        }
        attempts.push(json!({"target":expected["route"]["target"],"run":run,
            "context":expected["context"],"status":result["status"],"termination_confirmed":true}));
    }
    Ok(
        json!({"thread":plan["thread"],"settled":true,"attempts":attempts,
        "dispatch":{"name":dispatch_name,"status":observed["status"],"reason":observed["reason"],
        "attempted_entries":observed["attempted_entries"],"selected_entries":observed["selected_entries"]}}),
    )
}
pub fn inspect(workspace: &Path, name: &str) -> Result<Value> {
    let (space, root, record) = open(workspace, name)?;
    let current = control(&root)?;
    let rows: Vec<Value> = threads(&root)?
        .iter()
        .map(|path| {
            settled(&space, &record, path).unwrap_or_else(|error| {
                json!({
            "thread":path.file_name().and_then(|s| s.to_str()),"settled":false,
            "status":"running-or-unconfirmed","diagnostic":error.to_string()})
            })
        })
        .collect();
    Ok(
        json!({"session":record,"control":current,"threads":rows,"automatic_retries":0,
        "authority":"observation only; closing never releases references or deletes artifacts"}),
    )
}
