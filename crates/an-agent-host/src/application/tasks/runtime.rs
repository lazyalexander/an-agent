//! Exact ownership of the task-local control service, separate from VM execution.
use super::*;
use crate::execution::sandbox::{product_process, service_process};

pub fn record(service: &Path, id: &str, pid: u32) -> Result<Value> {
    let root = directory(service, id)?;
    let owner: Value = serde_json::from_slice(&store::read(&root.join("owner.json"), 65536)?)?;
    let parent = owner["pid"]
        .as_u64()
        .and_then(|p| u32::try_from(p).ok())
        .ok_or("controller owner")?;
    if service_process::identity(parent) != owner["identity"]
        || owner["identity"].is_null()
        || !service_process::child_of(pid, parent)
    {
        return Err("runtime must be an original controller child".into());
    }
    let identity = service_process::identity(pid);
    if identity.is_null() {
        return Err("runtime identity unavailable".into());
    }
    let record = json!({"controller":owner,"identity":identity});
    let pending = root.join(".runtime-owner.pending");
    store::write_new(&pending, &serde_json::to_vec(&record)?)?;
    fs::hard_link(&pending, root.join("runtime-owner.json"))?;
    fs::remove_file(pending)?;
    fs::File::open(&root)?.sync_all()?;
    Ok(json!({"recorded":true}))
}

pub(super) fn cleanup(root: &Path) -> Result<bool> {
    if !root.join("runtime-launch.json").exists() {
        return Ok(true);
    }
    let path = root.join("runtime-owner.json");
    if !path.exists() {
        return Ok(false);
    }
    let record: Value = serde_json::from_slice(&store::read(&path, 65536)?)?;
    let owner: Value = serde_json::from_slice(&store::read(&root.join("owner.json"), 65536)?)?;
    if record["controller"] != owner {
        return Err("runtime controller binding changed".into());
    }
    product_process::stop_control(&record["identity"])
}
