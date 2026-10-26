//! Release-bound native spool facade over qualified workspace execution.
//! This reviewed-tool profile is not hostile process-tree containment.
mod bundle;
mod lifecycle;
mod run;
use crate::{
    execution::{Result, store},
    file_job::regular,
};
pub use lifecycle::{load, show, unload, update_kit};
pub use run::{accept, reclaim, start};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn location(path: &Path) -> Result<PathBuf> {
    store::no_links(path)?;
    Ok(path.canonicalize()?)
}
fn state(root: &Path) -> Result<(Value, bundle::Bundle)> {
    let record: Value = serde_json::from_slice(&regular(&root.join("instance.json"), 65536)?)?;
    let bundle = bundle::read(&root.join("release"))?;
    if record["schema"] != "an-native-spool-instance-v1"
        || record["release"] != bundle.release
        || record["instance"]
            .as_str()
            .is_none_or(|id| uuid::Uuid::parse_str(id).is_err())
    {
        return Err("instance release binding mismatch".into());
    }
    Ok((record, bundle))
}
fn active(root: &Path) -> Result<(Value, bundle::Bundle)> {
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
