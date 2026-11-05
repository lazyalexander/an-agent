//! Content provenance is independent of legacy directory bindings and grants.
use super::*;

pub(super) fn inspect(bundle: &bundle::Bundle, kits: &Value) -> Value {
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
        copies.insert(path.clone(), json!({"label":"kit","sha256":digest,
            "origin":{"release":bundle.release,"path":path,"sha256":bundle.manifest.files.get(path)}}));
    }
    json!({"schema":"an-spool-content-labels-v1","baseline":baseline,"working_copies":copies,
        "unregistered_work_default":"kit","unregistered_work_scope":"attempt-working-files",
        "host_records":"protected-evidence","instructions":"protected-release-only",
        "execution_granted":false,"supported_edit_prefixes":["kit/"],
        "accepted_artifacts":"separate-custody","committed_state":"separate-custody"})
}
