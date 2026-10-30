//! Read-only release evidence; neither lineage nor domain compatibility is inferred.
use super::{Result, Value, bundle, json, requirements};
use std::{collections::BTreeSet, path::Path};

/// Compare verified package bytes, including requests this host cannot execute.
/// Supplying an instance's `release` directory compares its immutable baseline,
/// not its edited working kit or its checkpoint.
pub fn compare_releases(previous: &Path, candidate: &Path) -> Result<Value> {
    let before = bundle::inspect(previous)?;
    let after = bundle::inspect(candidate)?;
    let a = serde_json::to_value(&before.manifest)?;
    let b = serde_json::to_value(&after.manifest)?;
    let paths: BTreeSet<_> = before
        .manifest
        .files
        .keys()
        .chain(after.manifest.files.keys())
        .collect();
    let code: Vec<_> = paths
        .into_iter()
        .filter_map(|path| {
            let old = before.manifest.files.get(path);
            let new = after.manifest.files.get(path);
            (old != new).then(|| json!({"path":path,"before":old,"after":new}))
        })
        .collect();
    let same = before.release == after.release;
    let same_labels = a["name"] == b["name"] && a["version"] == b["version"];
    Ok(json!({"schema":"an-native-spool-release-diff-v1",
        "previous":summary(&before)?,"candidate":summary(&after)?,
        "same_release":same,"reused_coordinates":same_labels && !same,
        "changes":{"code":code,
            "manifest":fields(&a,&b,&["schema","name","version","entry","outputs","events"]),
            "requirements":fields(&a["requirements"],&b["requirements"],
                &["runtime","os","arch","interaction","gpu","inputs","editable","capabilities","dependencies"])},
        "review_required":!same,"execution_granted":false,"successor_authorized":false,
        "domain_compatibility":"not_evaluated","state_migration":"not_evaluated"}))
}

fn summary(bundle: &bundle::Bundle) -> Result<Value> {
    let refusal = requirements::validate(&bundle.manifest)
        .err()
        .map(|error| error.to_string())
        .or_else(|| (!cfg!(target_os = "macos")).then(|| "native execution requires macOS".into()));
    Ok(
        json!({"release":bundle.release,"manifest":serde_json::to_value(&bundle.manifest)?,
        "legacy_implicit_kit_editability":bundle.manifest.requirements.is_none(),
        "runtime_supported":refusal.is_none(),"runtime_refusal":refusal}),
    )
}

fn fields(before: &Value, after: &Value, names: &[&str]) -> Vec<Value> {
    names
        .iter()
        .copied()
        .filter(|name| before[*name] != after[*name])
        .map(|name| json!({"field":name,"before":before[name],"after":after[name]}))
        .collect()
}

#[cfg(test)]
#[path = "release_review_tests.rs"]
mod tests;
