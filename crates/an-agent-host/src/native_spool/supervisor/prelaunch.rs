//! Positive no-launch evidence closes a round, never grants payload custody.
use super::*;

pub(in super::super) fn settled(root: &Path, name: &str) -> Result<Value> {
    let (dir, record) = request(root, name)?;
    let hash = text(&record, "approval_sha256")?;
    let owner = read(&dir.join("owner.json"))?;
    if owner["request"] != hash || owner["pid"].as_u64().is_none_or(|pid| pid == 0) {
        return Err("prelaunch supervisor owner mismatch".into());
    }
    let response = read(&dir.join("completion.json"))?;
    let cancelled = json!({"status":"cancelled","executed":false,"process_started":false});
    if response != json!({"request":hash,"result":cancelled}) {
        return Err("supervisor has not confirmed prelaunch cancellation".into());
    }
    let run = dir.parent().ok_or("supervisor run directory")?;
    for marker in [
        "job",
        "start.json",
        "result.json",
        "input-references.json",
        "input-references-registered.json",
        "accepted.json",
        "reclaimed.json",
    ] {
        let path = run.join(marker);
        store::no_links(&path)?;
        if path.try_exists()? {
            return Err(
                "prelaunch cancellation conflicts with execution or custody evidence".into(),
            );
        }
    }
    Ok(json!({"schema":"an-native-prelaunch-cancellation-v1",
        "instance":record["plan"]["start"]["instance"],
        "release":record["plan"]["start"]["release"],"run":name,
        "request":hash,"status":"cancelled","process_started":false,
        "termination_confirmed":true,"custody_authorized":false}))
}
