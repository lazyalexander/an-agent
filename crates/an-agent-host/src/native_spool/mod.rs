//! Release-bound native spool facade over qualified workspace execution.
//! This reviewed-tool profile is not hostile process-tree containment.
mod artifacts;
pub use artifacts::artifacts;
mod bundle;
pub mod checkpoint;
mod event_contract;
pub mod events;
pub mod kit_proposal;
mod lifecycle;
mod reconcile;
mod release_review;
mod requirements;
pub use release_review::compare_releases;
mod round_control;
pub use reconcile::reconcile_accept;
pub use round_control::{pause, resume, suspend};
mod run;
mod run_reconcile;
pub use run_reconcile::reconcile_run;
mod status;
mod storage;
use crate::{
    execution::{Result, store},
    file_job::regular,
};
/// Legacy module alias; wire schemas and stored tickets retain their identities.
pub use kit_proposal as weaver;
pub use lifecycle::{load, reset_kit, show, unload, update_kit};
pub use run::{accept, reclaim, start, start_state};
use serde_json::{Value, json};
pub use status::status;
use std::{
    fs,
    path::{Path, PathBuf},
};
pub use storage::{reclaim_snapshots, storage};

fn location(path: &Path) -> Result<PathBuf> {
    store::no_links(path)?;
    Ok(path.canonicalize()?)
}
fn state(root: &Path) -> Result<(Value, bundle::Bundle)> {
    let record: Value = serde_json::from_slice(&regular(&root.join("instance.json"), 65536)?)?;
    let bundle = bundle::read(&root.join("release"))?;
    if !matches!(
        record["schema"].as_str(),
        Some("an-native-spool-instance-v1" | "an-native-spool-instance-v2")
    ) || record["release"] != bundle.release
        || record["instance"]
            .as_str()
            .is_none_or(|id| uuid::Uuid::parse_str(id).is_err())
    {
        return Err("instance release binding mismatch".into());
    }
    Ok((record, bundle))
}
fn active(root: &Path) -> Result<(Value, bundle::Bundle)> {
    let (instance, bundle) = loaded(root)?;
    round_control::admitted(root, &instance)?;
    Ok((instance, bundle))
}
fn loaded(root: &Path) -> Result<(Value, bundle::Bundle)> {
    if root.join("unloaded.json").exists() {
        return Err("spool instance is unloaded".into());
    }
    state(root)
}
fn approval(plan: &Value, supplied: Option<&str>) -> Result<(String, bool)> {
    let hash = store::digest(&serde_json::to_vec(plan)?);
    if supplied.is_some_and(|s| s != hash) {
        return Err("spool approval mismatch".into());
    }
    Ok((hash, supplied.is_some()))
}
fn write_json(path: &Path, value: &Value) -> Result<()> {
    store::write_new(path, &serde_json::to_vec_pretty(value)?)
}
fn readonly(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o400))?;
    }
    fs::File::open(path)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests;
