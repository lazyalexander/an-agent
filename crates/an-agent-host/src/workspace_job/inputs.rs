//! Pinned artifact references stay on the host; workloads receive selected copies.
use super::profile::ArtifactInput;
use crate::{
    artifact_versions,
    execution::{Result, store},
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::Path};

pub(super) fn bind(
    requests: &[ArtifactInput],
    inputs: &[Value],
    roots: &[std::path::PathBuf],
    work: &Path,
) -> Result<Vec<Value>> {
    if requests.len() > 16 {
        return Err("too many artifact inputs".into());
    }
    let mut paths = BTreeSet::new();
    let mut result = Vec::new();
    for request in requests {
        store::no_links(&request.path)?;
        let path = request.path.canonicalize()?;
        if !paths.insert(path.clone()) {
            return Err("duplicate artifact input".into());
        }
        let selected = inputs
            .iter()
            .find(|i| i["path"] == json!(path))
            .ok_or("artifact input must be a selected read-only file")?;
        let catalog = request.catalog.canonicalize()?;
        if catalog.starts_with(work)
            || roots
                .iter()
                .any(|r| catalog.starts_with(r) || r.starts_with(&catalog))
        {
            return Err(
                "artifact catalog must remain outside workload read and write roots".into(),
            );
        }
        let (bytes, binding) = artifact_versions::read(&catalog, &request.revision)?;
        if selected["sha256"] != store::digest(&bytes) {
            return Err("selected file does not match pinned artifact revision".into());
        }
        result.push(json!({"path":path,"catalog":catalog,"binding":binding}));
    }
    Ok(result)
}

pub(super) fn verify(plan: &Value) -> Result<()> {
    if let Some(inputs) = plan["artifact_inputs"].as_array() {
        for item in inputs {
            let (_, observed) = artifact_versions::read(
                Path::new(item["catalog"].as_str().ok_or("missing catalog")?),
                item["binding"]["revision"]
                    .as_str()
                    .ok_or("missing pinned revision")?,
            )?;
            if observed != item["binding"] {
                return Err("pinned artifact binding changed".into());
            }
        }
    }
    Ok(())
}
