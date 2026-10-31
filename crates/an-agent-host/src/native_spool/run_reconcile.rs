//! Explicit result-record repair; settled workspace evidence is never inferred.
use super::*;

pub fn reconcile_run(path: &Path, name: &str, approved: Option<&str>) -> Result<Value> {
    bundle::identifier(name)?;
    let root = location(path)?;
    let _lock = store::lock(&root)?;
    let (instance, _) = state(&root)?;
    let dir = root.join("runs").join(name);
    store::no_links(&dir)?;
    let owner: Value = serde_json::from_slice(&regular(&dir.join("spool-owner.json"), 65536)?)?;
    if owner != json!({"instance":instance["instance"],"release":instance["release"],"run":name}) {
        return Err("run reconciliation ownership mismatch".into());
    }
    let start_bytes = regular(&dir.join("start.json"), 2 * 1024 * 1024)?;
    let start: Value = serde_json::from_slice(&start_bytes)?;
    let mut selected = None;
    let mut ignored = Vec::new();
    for entry in fs::read_dir(dir.join("job/attempts"))? {
        let entry = entry?;
        let name = entry.file_name();
        let kind = entry.file_type()?;
        // Finder metadata is not an attempt. Do not follow links or silently
        // ignore other entries that could hide an incomplete/ambiguous attempt.
        if name == ".DS_Store" && kind.is_file() {
            ignored.push(".DS_Store");
            continue;
        }
        let id = name
            .to_str()
            .ok_or_else(|| format!("unexpected attempt entry: {name:?}"))?;
        if !kind.is_dir()
            || uuid::Uuid::parse_str(id)
                .ok()
                .is_none_or(|uuid| uuid.to_string() != id)
        {
            return Err(format!(
                "unexpected attempt entry: {name:?}; expected a canonical UUID directory"
            )
            .into());
        }
        if let Some((prior, _)) = &selected {
            return Err(format!(
                "run reconciliation requires exactly one attempt; found {prior} and {id}"
            )
            .into());
        }
        selected = Some((id.to_owned(), entry.path()));
    }
    let (id, attempt) = selected.ok_or("missing execution attempt")?;
    let (plan, receipt) = crate::workspace_job::verified_execution_record(&dir.join("job"), &id)?;
    if start["schema"] != "an-native-spool-start-v1"
        || start["instance"] != instance["instance"]
        || start["release"] != instance["release"]
        || start["run"] != name
        || start["workspace"] != plan
        || !matches!(
            receipt["status"].as_str(),
            Some("succeeded" | "failed" | "cancelled" | "timed_out")
        )
        || plan["profile_sha256"] != store::digest(&regular(&dir.join("profile.json"), 65536)?)
    {
        return Err("run reconciliation start/profile binding mismatch".into());
    }
    let intent: Value = serde_json::from_slice(&regular(&attempt.join("intent.json"), 65536)?)?;
    if intent["attempt"] != id
        || intent["approval_sha256"] != store::digest(&serde_json::to_vec(&plan)?)
    {
        return Err("run reconciliation workspace intent mismatch".into());
    }
    let result = json!({"schema":"an-native-spool-run-v1","instance":instance["instance"],
        "release":instance["release"],"run":name,"profile_sha256":plan["profile_sha256"],
        "status":receipt["status"],"workspace":receipt});
    let target = dir.join("result.json");
    let existing = match fs::symlink_metadata(&target) {
        Ok(_) => {
            let prior: Value = serde_json::from_slice(&regular(&target, 2 * 1024 * 1024)?)?;
            if prior != result {
                return Err("existing native result conflicts with workspace evidence".into());
            }
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.into()),
    };
    let mut plan = json!({"schema":"an-native-spool-reconcile-run-v1","binding":result,
        "start_sha256":store::digest(&start_bytes),"execution_retried":false,"artifacts_published":false});
    if !ignored.is_empty() {
        plan["ignored_attempt_entries"] = json!(ignored);
    }
    let (hash, execute) = approval(&plan, approved)?;
    if !execute {
        return Ok(
            json!({"plan":plan,"approval_sha256":hash,"executed":false,"already_recorded":existing}),
        );
    }
    if !existing {
        write_json(&target, &result)?;
    }
    Ok(
        json!({"status":"succeeded","binding":result,"already_recorded":existing,
        "execution_retried":false,"artifacts_published":false}),
    )
}
