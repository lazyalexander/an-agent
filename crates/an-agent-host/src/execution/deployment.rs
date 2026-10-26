//! Immutable release validation and registration; no execution grant.
use super::{
    Result, publication,
    store::{directory, load, lock, write_new},
};
use an_agent_core::det_seam::Entropy;
use an_agent_spool::spool::Registry;
use serde_json::{Value, json};
use std::{fs, path::Path};

/// Pure validation; accepting a deployment does not grant execution.
pub fn check(folder: &Path) -> Result<Value> {
    let deployment = load(folder)?;
    Ok(
        json!({"release":deployment.spec.sha256,"name":deployment.spec.name,"version":deployment.spec.version,"execution_granted":false}),
    )
}

/// Publish a snapshot of exactly two runtime files, not a source/build archive.
/// The YAML digest identifies the release and transitively binds executable bytes.
pub fn register(folder: &Path, root: &Path) -> Result<Value> {
    let deployment = load(folder)?;
    let spec = &deployment.spec;
    directory(root)?;
    let _lock = lock(root)?;
    directory(&root.join("registry"))?;
    directory(&root.join("releases"))?;
    let registry = Registry::open(root.join("registry"))?;
    for prior in registry.versions(&spec.name)? {
        if prior.version == spec.version && prior.sha256 != spec.sha256 {
            return Err("name/version already registered with different bytes".into());
        }
    }
    let target = root.join("releases").join(&spec.sha256);
    if !target.exists() {
        let staging = root
            .join("releases")
            .join(format!("{}.pending", Entropy::os().uuid_v4()));
        fs::create_dir(&staging)?;
        write_new(&staging.join("spool.yaml"), spec.body.as_bytes())?;
        let published = publication::publish(
            &staging.join("program"),
            &deployment.program,
            &std::sync::atomic::AtomicBool::new(false),
        )?;
        if published.stopped.is_some() {
            return Err(format!(
                "registration publisher did not complete: {}",
                published.evidence
            )
            .into());
        }
        fs::rename(staging, &target)?;
    }
    let retained = load(&target)?;
    if retained.spec.sha256 != spec.sha256 {
        return Err("retained release mismatch".into());
    }
    registry.publish(&spec.body)?;
    Ok(json!({"registered":spec.sha256,"activated":false}))
}
