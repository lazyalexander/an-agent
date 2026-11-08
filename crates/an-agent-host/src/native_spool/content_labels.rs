//! Content provenance is independent of legacy directory bindings and grants.
use super::*;

pub(super) fn inspect(root: &Path, bundle: &bundle::Bundle, kits: &Value) -> Result<Value> {
    let mut touched = std::collections::BTreeSet::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("edit-") && name.ends_with(".json") {
            let edit: Value = serde_json::from_slice(&regular(&entry.path(), 1024 * 1024)?)?;
            if let Some(path) = edit["kit"].as_str() {
                touched.insert(path.to_owned());
            }
        }
    }
    let mut baseline = serde_json::Map::new();
    for (path, bytes) in &bundle.payload {
        baseline.insert(
            path.clone(),
            json!({"label":"silk","sha256":store::digest(bytes)}),
        );
    }
    baseline.insert(
        bundle::FILE.into(),
        json!({"label":"silk","sha256":store::digest(&bundle.bytes)}),
    );
    let mut copies = serde_json::Map::new();
    for (path, digest) in kits.as_object().into_iter().flatten() {
        let matches_baseline = bundle
            .manifest
            .files
            .get(path)
            .is_some_and(|base| digest.as_str() == Some(base.as_str()));
        let modified = touched.contains(path) || !matches_baseline;
        copies.insert(path.clone(), json!({"label":if modified { "kit" } else { "silk" },"sha256":digest,
            "modified":modified,"matches_baseline":matches_baseline,
            "requires_acceptance":modified,
            "origin":{"release":bundle.release,"path":path,"sha256":bundle.manifest.files.get(path)}}));
    }
    Ok(
        json!({"schema":"an-spool-content-labels-v1","baseline":baseline,"working_copies":copies,
        "unregistered_work_default":"kit","unregistered_work_scope":"attempt-working-files",
        "host_records":"protected-evidence","instructions":"protected-release-only",
        "execution_granted":false,"supported_edit_prefixes":["kit/"],
        "accepted_artifacts":"separate-custody","committed_state":"separate-custody"}),
    )
}
