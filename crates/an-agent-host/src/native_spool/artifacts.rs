//! Read-only discovery of formally accepted versions, never scratch promotion.
use super::*;

/// Observe incoming ownership without taking a producer or consumer control lock.
pub fn artifact_references(path: &Path, name: &str, output: &str) -> Result<Value> {
    let report = artifact_output(path, name, output)?;
    let selected = &report["output"];
    let mut observation = crate::artifact_versions::references::observe(
        Path::new(report["catalog"].as_str().ok_or("catalog")?),
        selected["binding"]["revision"].as_str().ok_or("revision")?,
    )?;
    observation["producer"] = json!({"instance":report["instance"],"release":report["release"],
        "run":name,"output":output});
    Ok(observation)
}

pub fn artifacts(path: &Path, name: &str) -> Result<Value> {
    let root = location(path)?;
    let _lock = store::lock(&root)?;
    let mut report = verified(&root, name, true)?.0;
    let catalog = Path::new(report["catalog"].as_str().ok_or("catalog")?);
    let mut admission = serde_json::Map::new();
    for output in report["outputs"].as_array().ok_or("outputs")? {
        admission.insert(
            output["path"].as_str().ok_or("output")?.into(),
            crate::artifact_versions::admission::status(
                catalog,
                output["binding"]["revision"].as_str().ok_or("revision")?,
            )?,
        );
    }
    // Keep the original accepted report stable for causal/consumption receipts.
    report["admission"] = json!(admission);
    Ok(report)
}

/// Verify one payload and the complete acceptance inventory. No custody authority.
pub fn artifact_output(path: &Path, name: &str, output: &str) -> Result<Value> {
    Ok(verify_scope(&location(path)?, name, true, Some(output), false)?.0)
}

pub(super) fn verified(root: &Path, name: &str, require_bound: bool) -> Result<(Value, Value)> {
    verify_scope(root, name, require_bound, None, false)
}

pub(super) fn selected_metadata(root: &Path, name: &str, output: &str) -> Result<Value> {
    Ok(verify_scope(root, name, true, Some(output), true)?.0)
}

fn verify_scope(
    root: &Path,
    name: &str,
    require_bound: bool,
    selected: Option<&str>,
    metadata_only: bool,
) -> Result<(Value, Value)> {
    let settled = run::settled(root, name)?;
    if settled["status"] != "succeeded" {
        return Err("only successful accepted runs have formal artifacts".into());
    }
    let dir = root.join("runs").join(name);

    let execution = regular(
        &dir.join("job/attempts")
            .join(settled["workspace"]["attempt"].as_str().ok_or("attempt")?)
            .join("execution.json"),
        2 * 1024 * 1024,
    )?;
    if store::digest(&execution) != settled["workspace"]["execution_sha256"] {
        return Err("accepted execution evidence changed".into());
    }
    let accepted: Value = serde_json::from_slice(&regular(
        &dir.join("job/attempts")
            .join(settled["workspace"]["attempt"].as_str().ok_or("attempt")?)
            .join("accepted.json"),
        65536,
    )?)?;
    if accepted["schema"] != "an-workspace-accepted-v1" || accepted["status"] != "succeeded" {
        return Err("formal workspace acceptance missing".into());
    }
    if require_bound {
        let bound: Value = serde_json::from_slice(&regular(&dir.join("accepted.json"), 65536)?)?;
        if bound
            != json!({"status":"succeeded","instance":settled["instance"],"release":settled["release"],
            "run":name,"operation":"accept","result":accepted})
        {
            return Err("formal artifact ownership mismatch".into());
        }
    }
    let intent: Value = serde_json::from_slice(&regular(
        &dir.join("job/attempts")
            .join(settled["workspace"]["attempt"].as_str().ok_or("attempt")?)
            .join("accept-intent.json"),
        65536,
    )?)?;
    let receipt_hash = store::digest(&regular(
        &dir.join("job/attempts")
            .join(settled["workspace"]["attempt"].as_str().ok_or("attempt")?)
            .join("receipt.json"),
        2 * 1024 * 1024,
    )?);
    if intent["schema"] != "an-workspace-accept-v1"
        || intent["attempt"] != settled["workspace"]["attempt"]
        || intent["receipt_sha256"] != receipt_hash
        || accepted["approval_sha256"] != store::digest(&serde_json::to_vec(&intent)?)
    {
        return Err("formal acceptance evidence mismatch".into());
    }
    let (_, bundle) = state(root)?;
    let outputs = accepted["outputs"]
        .as_array()
        .ok_or("missing accepted outputs")?;
    let candidates = intent["outputs"]
        .as_array()
        .ok_or("missing accepted inventory")?;
    if outputs.len() != bundle.manifest.outputs.len() || candidates.len() != outputs.len() {
        return Err("formal output inventory mismatch".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut selected_output = None;
    let catalog = dir.join("job/artifacts");
    for output in outputs {
        let name = output["path"].as_str().ok_or("output path")?;
        if !bundle.manifest.outputs.iter().any(|p| p == name) || !seen.insert(name) {
            return Err("undeclared or duplicate artifact".into());
        }
        let revision = output["binding"]["revision"].as_str().ok_or("revision")?;
        let version = if selected.is_none() || selected == Some(name) {
            if selected.is_some() {
                selected_output = Some(output);
            }
            if selected.is_none() {
                crate::artifact_versions::registered::verify_or_deleted(&catalog, revision)?
            } else if metadata_only {
                crate::artifact_versions::metadata(&catalog, revision)?
            } else {
                crate::artifact_versions::registered::verify(&catalog, revision)?
            }
        } else {
            crate::artifact_versions::metadata(&catalog, revision)?
        };
        let candidate = candidates
            .iter()
            .find(|p| p["path"] == name)
            .ok_or("unaccepted artifact")?;
        let binary = bundle.manifest.binary_outputs.iter().any(|p| p == name);
        if version["reader_profile"] != intent["reader_profile"]
            || version["reader_profile"] != output["binding"]["reader_profile"]
            || (version["encoding"] == "bytes") != binary
            || (candidate["encoding"] == "bytes") != binary
            || (output["binding"]["encoding"] == "bytes") != binary
            || version["content_sha256"] != output["binding"]["content_sha256"]
            || version["bytes"] != output["binding"]["bytes"]
            || candidate["sha256"] != version["content_sha256"]
            || candidate["bytes"] != version["bytes"]
            || version["source"]["kind"] != "workspace-output"
            || version["source"]["profile_sha256"] != settled["profile_sha256"]
            || version["source"]["attempt"] != settled["workspace"]["attempt"]
            || version["source"]["receipt_sha256"] != receipt_hash
            || version["source"]["path"] != name
        {
            return Err("artifact version or provenance mismatch".into());
        }
    }
    if selected.is_some() {
        let output = selected_output.ok_or("unknown accepted output")?;
        return Ok((
            json!({"schema":"an-native-selected-artifact-v1","instance":settled["instance"],
            "release":settled["release"],"run":name,"catalog":catalog,"output":output,
            "payload_verified":!metadata_only,"inventory_metadata_verified":true}),
            accepted,
        ));
    }
    let report = json!({"schema":"an-native-spool-artifacts-v1","instance":settled["instance"],
        "release":settled["release"],"run":name,"catalog":dir.join("job/artifacts"),"outputs":outputs});
    Ok((report, accepted))
}
