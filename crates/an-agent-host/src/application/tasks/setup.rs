//! Preparation ownership reuses the original Cocoon claim and stop receipt.
use super::*;

pub fn bind(service: &Path, id: &str) -> Result<Value> {
    let root = directory(service, id)?;
    let setup: Value = serde_json::from_slice(&store::read(&root.join("setup.json"), 65536)?)?;
    let spec = root.join("extract-spec.json");
    let bytes = store::read(&spec, 131072)?;
    let selected: Value = serde_json::from_slice(&bytes)?;
    let profile: Value = serde_json::from_slice(&store::read(
        Path::new(selected["transport"].as_str().ok_or("setup transport")?),
        65536,
    )?)?;
    if profile["vm"] != setup["vm"] {
        return Err("setup VM binding changed".into());
    }
    crate::cocoon::job::bind(&root, &spec, &store::digest(&bytes), id)?;
    Ok(json!({"bound":true}))
}

pub(super) fn release(root: &Path) -> Result<bool> {
    // Only new controllers publish this fence before any setup effect.
    if !versioned(root)? {
        return Ok(false);
    }
    if !root.join("start-intent.json").exists() {
        return Ok(true);
    }
    let released = crate::cocoon::job::release(root)?;
    Ok(released["termination_confirmed"] == true)
}

pub fn release_original(service: &Path, id: &str) -> Result<Value> {
    Ok(json!({"termination_confirmed":release(&directory(service, id)?)?}))
}

pub(super) fn versioned(root: &Path) -> Result<bool> {
    let path = root.join("recovery-version.json");
    if !path.exists() {
        return Ok(false);
    }
    let record: Value = serde_json::from_slice(&store::read(&path, 65536)?)?;
    if record != json!({"version":1}) {
        return Err("unsupported delivery recovery version".into());
    }
    Ok(true)
}

pub fn settle_image(service: &Path, id: &str) -> Result<Value> {
    Ok(json!({"settled":crate::application::images::settle_task_branch(&directory(service,id)?)?}))
}
