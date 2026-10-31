//! Explicit summary reconstruction from complete execution evidence, never replay.
use super::*;

pub fn reconcile_dispatch(path: &Path, name: &str, approved: Option<&str>) -> Result<Value> {
    let space = Space::open(path)?;
    native::bundle::identifier(name)?;
    let dir = space.root.join("dispatches").join(name);
    store::no_links(&dir)?;
    let intent = dispatch::recorded(&space, &dir)?;
    let selected = intent["plan"]["entries"]
        .as_array()
        .ok_or("dispatch entries")?;
    if !(1..=16).contains(&selected.len()) {
        return Err("invalid dispatch selection count".into());
    }
    // Never hold the workspace lock while acquiring instance locks.
    let mut proofs = Vec::new();
    for (index, selection) in selected.iter().enumerate() {
        let expected = &selection["plan"];
        let root = Path::new(text(&expected["route"]["target"], "root")?);
        let _instance = store::lock(root)?;
        let _workspace = store::lock(&space.root)?;
        let event = space.event(text(&expected["event"], "id")?)?;
        let route = space.route(text(&expected["route"], "name")?)?;
        let admitted = delivery::intent(&space, &event, &route)?;
        if admitted["plan"] != *expected {
            return Err("dispatch and delivery intent differ".into());
        }
        let entry: Value = serde_json::from_slice(&regular(
            &dir.join(format!("entry-{index}.json")),
            4 * 1024 * 1024,
        )?)?;
        let run = text(&expected["context"], "delivery")?;
        let result = native::run::settled(root, run)?;
        let start: Value = serde_json::from_slice(&regular(
            &root.join("runs").join(run).join("start.json"),
            2 * 1024 * 1024,
        )?)?;
        if entry["index"] != index
            || entry["dispatch_sha256"] != intent["approval_sha256"]
            || entry["result"]["context"] != expected["context"]
            || entry["result"]["status"] != "succeeded"
            || entry["result"]["result"] != result
            || result["status"] != "succeeded"
            || result["workspace"]["termination_confirmed"] != true
            || start != expected["start"]
        {
            return Err("dispatch entry lacks matching confirmed successful execution".into());
        }
        proofs.push(entry);
    }
    let summary = json!({"schema":"an-native-event-dispatch-result-v1","status":"succeeded","reason":"completed",
        "dispatch_sha256":intent["approval_sha256"],"attempted_entries":selected.len(),"selected_entries":selected.len(),
        "executed":true,"automatic_retries":0,"consumed":false});
    let plan = json!({"operation":"reconcile-dispatch-summary","intent":intent,"entries":proofs,
        "summary":summary,"execution_granted":false});
    let (digest, execute) = native::approval(&plan, approved)?;
    let _lock = store::lock(&space.root)?;
    if dir.join("result.json").exists() {
        let prior: Value = serde_json::from_slice(&regular(&dir.join("result.json"), 65536)?)?;
        if prior != summary {
            return Err("existing dispatch summary conflicts; never overwrite".into());
        }
        return Ok(
            json!({"status":"succeeded","executed":false,"already_recorded":true,"automatic_retries":0}),
        );
    }
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":digest,"executed":false}));
    }
    write_json(&dir.join("result.json"), &summary)?;
    fs::File::open(&dir)?.sync_all()?;
    Ok(
        json!({"status":"succeeded","executed":false,"summary_reconstructed":true,"automatic_retries":0,"consumed":false}),
    )
}
