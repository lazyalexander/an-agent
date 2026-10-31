//! Close one formally accepted output to new input acquisition, without deleting it.
use super::*;

pub fn retire_artifact(
    path: &Path,
    run: &str,
    output: &str,
    approved: Option<&str>,
) -> Result<Value> {
    let root = location(path)?;
    let _lock = store::lock(&root)?;
    let (report, accepted) = artifacts::verified(&root, run, true)?;
    let selected = report["outputs"]
        .as_array()
        .ok_or("accepted outputs")?
        .iter()
        .find(|p| p["path"] == output)
        .ok_or("retirement requires one formally accepted declared output")?;
    crate::artifact_versions::admission::retire(
        Path::new(report["catalog"].as_str().ok_or("catalog")?),
        selected["binding"]["revision"].as_str().ok_or("revision")?,
        json!({"kind":"native-spool-output","instance":report["instance"],"release":report["release"],
            "run":run,"output":output,"accepted_sha256":store::digest(&serde_json::to_vec(&accepted)?)}),
        approved,
    )
}
