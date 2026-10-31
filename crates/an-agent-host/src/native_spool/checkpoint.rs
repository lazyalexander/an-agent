//! Explicit opaque state over existing immutable artifacts and instance ownership.
use super::*;
use crate::artifact_versions;

fn catalog(root: &Path) -> PathBuf {
    root.join("checkpoints/catalog")
}

fn metadata(root: &Path) -> Result<(Value, String)> {
    let (instance, bundle) = state(root)?;
    let metadata: Value =
        serde_json::from_slice(&regular(&root.join("checkpoints/state.json"), 65536)?)?;
    let output = metadata["output"].as_str().ok_or("missing state output")?;
    if metadata["schema"] != "an-native-spool-state-v1"
        || metadata["instance"] != instance["instance"]
        || metadata["release"] != instance["release"]
        || !bundle.manifest.outputs.iter().any(|p| p == output)
        || bundle.manifest.binary_outputs.iter().any(|p| p == output)
    {
        return Err("state ownership or output declaration mismatch".into());
    }
    let head = String::from_utf8(regular(&root.join("checkpoints/HEAD"), 64)?)?;
    if !store::valid_digest(&head) {
        return Err("invalid state head".into());
    }
    Ok((metadata, head))
}

pub(super) fn head_selection(root: &Path) -> Result<Value> {
    let (_, head) = metadata(root)?;
    selection(root, &head)
}

/// Select only a committed ancestor, never an orphan published before HEAD moved.
pub(super) fn selection(root: &Path, revision: &str) -> Result<Value> {
    if !store::valid_digest(revision) {
        return Err("state revision must be a full digest".into());
    }
    let (metadata, mut cursor) = metadata(root)?;
    for _ in 0..1024 {
        let (_, binding) = artifact_versions::read(&catalog(root), &cursor)?;
        if binding["source"]["instance"] != metadata["instance"]
            || binding["source"]["release"] != metadata["release"]
            || binding["source"]["output"] != metadata["output"]
            || binding.get("encoding").is_some()
        {
            return Err("state revision ownership mismatch".into());
        }
        if cursor == revision {
            return Ok(json!({"catalog":catalog(root),"binding":binding,
                "path":catalog(root).join("revisions").join(revision).join("content.txt"),
                "output":metadata["output"]}));
        }
        let Some(parent) = binding["parent"].as_str() else {
            break;
        };
        cursor = parent.into();
    }
    Err("state revision is not in bounded committed history".into())
}

/// Seed state explicitly; the host interprets no domain fields or transitions.
pub fn init(path: &Path, output: &str, file: &Path, approved: Option<&str>) -> Result<Value> {
    let root = location(path)?;
    let _lock = store::lock(&root)?;
    let (instance, bundle) = active(&root)?;
    if root.join("checkpoints").exists() {
        return Err("state already initialized".into());
    }
    if !bundle.manifest.outputs.iter().any(|p| p == output)
        || bundle.manifest.binary_outputs.iter().any(|p| p == output)
    {
        return Err("state must select a declared text output".into());
    }
    let bytes = regular(file, 65536)?;
    std::str::from_utf8(&bytes)?;
    let mut plan = json!({"schema":"an-native-spool-state-init-v1","instance":instance["instance"],
        "release":bundle.release,"output":output,"content_sha256":store::digest(&bytes)});
    round_control::bind(&mut plan, &root, &instance)?;
    let (hash, execute) = approval(&plan, approved)?;
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":hash,"executed":false}));
    }
    let pending = root.join("checkpoints.pending");
    fs::create_dir(&pending)?;
    let source = json!({"kind":"native-spool-state-init","instance":instance["instance"],
        "release":bundle.release,"output":output,"approval_sha256":hash});
    let first = artifact_versions::publish(&pending.join("catalog"), &bytes, None, source)?;
    write_json(
        &pending.join("state.json"),
        &json!({"schema":"an-native-spool-state-v1",
        "instance":instance["instance"],"release":bundle.release,"output":output}),
    )?;
    store::write_new(
        &pending.join("HEAD"),
        first["revision"].as_str().ok_or("revision")?.as_bytes(),
    )?;
    fs::rename(pending, root.join("checkpoints"))?;
    Ok(json!({"status":"succeeded","instance":instance["instance"],"state":first}))
}

/// Inspect a fixed committed state; an unloaded instance remains readable.
pub fn inspect(path: &Path, revision: Option<&str>) -> Result<Value> {
    let root = location(path)?;
    let _lock = store::lock(&root)?;
    let (metadata, head) = metadata(&root)?;
    let selected = selection(&root, revision.unwrap_or(&head))?;
    Ok(json!({"metadata":metadata,"head":head,"selected":selected}))
}

/// Commit only an accepted output of a run explicitly started from EXPECTED.
pub fn commit(path: &Path, expected: &str, name: &str, approved: Option<&str>) -> Result<Value> {
    let root = location(path)?;
    let _lock = store::lock(&root)?;
    let (instance, bundle) = active(&root)?;
    let (_, head) = metadata(&root)?;
    let selected = selection(&root, expected)?;
    let (artifacts, _) = super::artifacts::verified(&root, name, true)?;
    let dir = root.join("runs").join(name);
    let start: Value = serde_json::from_slice(&regular(&dir.join("start.json"), 2 * 1024 * 1024)?)?;
    let result = run::settled(&root, name)?;
    let attempt = result["workspace"]["attempt"].as_str().ok_or("attempt")?;
    let workspace: Value = serde_json::from_slice(&regular(
        &dir.join("job/attempts").join(attempt).join("plan.json"),
        2 * 1024 * 1024,
    )?)?;
    let inputs = super::inputs::commit_bindings(&bundle, &start, &selected)?;
    if start["schema"] != "an-native-spool-start-v1"
        || start["instance"] != instance["instance"]
        || start["release"] != instance["release"]
        || start["run"] != name
        || start["state_input"] != selected
        || start["workspace"] != workspace
        || workspace["artifact_inputs"] != inputs
        || result["workspace"]["status"] != "succeeded"
    {
        return Err("run did not consume this exact committed state".into());
    }
    let output = artifacts["outputs"]
        .as_array()
        .ok_or("outputs")?
        .iter()
        .find(|p| p["path"] == selected["output"])
        .ok_or("missing accepted state output")?;
    let revision = output["binding"]["revision"]
        .as_str()
        .ok_or("output revision")?;
    let (bytes, binding) = artifact_versions::read(
        Path::new(artifacts["catalog"].as_str().ok_or("artifact catalog")?),
        revision,
    )?;
    let mut plan = json!({"schema":"an-native-spool-state-commit-v1","instance":instance["instance"],
        "release":instance["release"],"expected":expected,"run":name,"attempt":attempt,
        "output":selected["output"],"artifact":binding});
    round_control::bind(&mut plan, &root, &instance)?;
    let (hash, execute) = approval(&plan, approved)?;
    if head != expected {
        let (_, current) = artifact_versions::read(&catalog(&root), &head)?;
        if current["parent"] == expected && current["source"]["approval_sha256"] == hash {
            return Ok(json!({"status":"succeeded","revision":head,"already_committed":true}));
        }
        return Ok(
            json!({"status":"conflict","expected":expected,"actual":head,"automatic_retries":0}),
        );
    }
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":hash,"executed":false}));
    }
    let checkpoints = root.join("checkpoints");
    let intent = checkpoints.join(format!("commit-{hash}.json"));
    if intent.exists() {
        let prior: Value = serde_json::from_slice(&regular(&intent, 65536)?)?;
        if prior != plan {
            return Err("state commit intent changed".into());
        }
    } else {
        write_json(&intent, &plan)?;
    }
    let next = artifact_versions::publish(
        &catalog(&root),
        &bytes,
        Some(expected.into()),
        json!({"kind":"native-spool-state-commit","instance":instance["instance"],
            "release":instance["release"],"output":selected["output"],"run":name,
            "attempt":attempt,"artifact":binding,"approval_sha256":hash}),
    )?;
    let revision = next["revision"].as_str().ok_or("revision")?;
    let pending = checkpoints.join(format!("HEAD-{hash}.pending"));
    if pending.exists() {
        if regular(&pending, 64)? != revision.as_bytes() {
            return Err("state head staging conflicts".into());
        }
    } else {
        store::write_new(&pending, revision.as_bytes())?;
    }
    fs::rename(pending, checkpoints.join("HEAD"))?;
    Ok(
        json!({"status":"succeeded","parent":expected,"revision":revision,"already_committed":false}),
    )
}
