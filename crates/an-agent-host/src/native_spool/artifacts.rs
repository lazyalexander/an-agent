//! Read-only discovery of formally accepted versions, never scratch promotion.
use super::*;

pub fn artifacts(path: &Path, name: &str) -> Result<Value> {
    let root = location(path)?;
    let _lock = store::lock(&root)?;
    let settled = run::settled(&root, name)?;
    if settled["status"] != "succeeded" {
        return Err("only successful accepted runs have formal artifacts".into());
    }
    let dir = root.join("runs").join(name);
    let bound: Value = serde_json::from_slice(&regular(&dir.join("accepted.json"), 65536)?)?;
    let accepted: Value = serde_json::from_slice(&regular(
        &dir.join("job/attempts")
            .join(settled["workspace"]["attempt"].as_str().ok_or("attempt")?)
            .join("accepted.json"),
        65536,
    )?)?;
    if bound["status"] != "succeeded"
        || bound["operation"] != "accept"
        || bound["instance"] != settled["instance"]
        || bound["release"] != settled["release"]
        || bound["run"] != name
        || bound["result"] != accepted
        || accepted["schema"] != "an-workspace-accepted-v1"
        || accepted["status"] != "succeeded"
    {
        return Err("formal artifact ownership mismatch".into());
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
    let (_, bundle) = state(&root)?;
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
    for output in outputs {
        let name = output["path"].as_str().ok_or("output path")?;
        if !bundle.manifest.outputs.iter().any(|p| p == name) || !seen.insert(name) {
            return Err("undeclared or duplicate artifact".into());
        }
        let (_, version) = crate::artifact_versions::read(
            &dir.join("job/artifacts"),
            output["binding"]["revision"].as_str().ok_or("revision")?,
        )?;
        let candidate = candidates
            .iter()
            .find(|p| p["path"] == name)
            .ok_or("unaccepted artifact")?;
        if version["content_sha256"] != output["binding"]["content_sha256"]
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
    Ok(
        json!({"schema":"an-native-spool-artifacts-v1","instance":settled["instance"],
        "release":settled["release"],"run":name,"catalog":dir.join("job/artifacts"),"outputs":outputs}),
    )
}
