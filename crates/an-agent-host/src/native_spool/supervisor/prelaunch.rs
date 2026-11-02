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

/// A submitted state request can precede the first source claim. A preview has no reservation.
pub(in crate::native_spool) fn checkpoint_reservations(
    root: &Path,
    binding: &Value,
) -> Result<Value> {
    let mut requests = std::collections::BTreeMap::new();
    for (index, entry) in fs::read_dir(root.join("runs"))?.enumerate() {
        if index >= 64 {
            return Err("native run census exceeds its bound".into());
        }
        let entry = entry?;
        store::no_links(&entry.path().join("supervisor"))?;
        if !entry.path().join("supervisor").try_exists()? {
            continue;
        }
        let name = entry.file_name().into_string().map_err(|_| "run name")?;
        let (_, record) = request(root, &name)?;
        if record["plan"]["start"]["state_input"]["binding"] != *binding {
            continue;
        }
        let request_hash = text(&record, "approval_sha256")?;
        let completed = completion(root, &name, request_hash).map_err(|e| {
            format!("submitted checkpoint reservation has no confirmed outcome; retain pins: {e}")
        })?;
        if completed.get("workspace").is_some() {
            let registration = input_references::registration(root, &name)?;
            let start = &record["plan"]["start"];
            if registration["start_sha256"] != store::digest(&serde_json::to_vec(start)?) {
                return Err(
                    "submitted checkpoint request differs from its admitted attempt".into(),
                );
            }
            let reference = registration["references"]
                .as_array()
                .ok_or("registered inputs")?
                .iter()
                .find(|r| {
                    r["catalog"] == start["state_input"]["catalog"] && r["binding"] == *binding
                })
                .ok_or("supervisor has no matching checkpoint reader")?;
            // The closed census already verified this source claim and its consumer receipt.
            if crate::artifact_versions::references::inspect(reference)?["release"].is_null() {
                return Err("submitted checkpoint reader is still held".into());
            }
        }
        requests.insert(
            name,
            json!({"request_sha256":request_hash,
            "result_sha256":store::digest(&serde_json::to_vec(&completed)?)}),
        );
    }
    Ok(json!(requests))
}
