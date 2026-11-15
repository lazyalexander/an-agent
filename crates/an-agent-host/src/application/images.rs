//! Host-only cold-disk management. The model receives references, never store credentials.
use crate::execution::{
    Result,
    sandbox::{product_process, service_process},
    store,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn uuid(value: &Value) -> Result<&str> {
    let value = value.as_str().ok_or("image operation identity required")?;
    if uuid::Uuid::parse_str(value)?.to_string() != value {
        return Err("noncanonical identity".into());
    }
    Ok(value)
}
pub(super) fn state_root(service: &Path, environment: &str) -> Result<PathBuf> {
    if !store::valid_digest(environment) {
        return Err("invalid image environment".into());
    }
    Ok(service
        .join("application/images")
        .join(environment)
        .join("cocoon"))
}
fn operation_root(service: &Path, environment: &str, id: &str) -> Result<PathBuf> {
    uuid(&json!(id))?;
    Ok(state_root(service, environment)?
        .parent()
        .ok_or("image namespace")?
        .join("operations")
        .join(id))
}
fn read(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&store::read(path, 131072)?)?)
}
fn runtime_manifest(environment: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&store::read(
        &environment.join("runtime.json"),
        2 * 1024 * 1024,
    )?)?)
}
fn runtime(service: &Path, environment: &str) -> Result<(PathBuf, Value)> {
    state_root(service, environment)?;
    let root = service.join("application/environments").join(environment);
    let verified = super::environment::validate_installed(&root)?;
    if verified["id"] != environment {
        return Err("image runtime changed".into());
    }
    Ok((root, verified["manifest"].clone()))
}
pub(super) fn capable(runtime: &Path, manifest: &Value) -> Result<bool> {
    let Some(component) = manifest["components"]["image_api"].as_str() else {
        return Ok(false);
    };
    let contract = read(&runtime.join(component))?;
    if contract
        != json!({"schema":"cocoon-image-api-v1","disk_images":true,"memory_restore_qualified":false})
    {
        return Err("unsupported image contract".into());
    }
    Ok(true)
}
fn binary(environment: &Path, manifest: &Value) -> Result<PathBuf> {
    Ok(environment.join(
        manifest["components"]["cocoon"]
            .as_str()
            .ok_or("Cocoon component")?,
    ))
}
fn query(binary: &Path, root: &Path, command: &str, argument: Option<&Path>) -> Result<Value> {
    product_process::image_query(binary, root, command, argument)
}
fn image(binary: &Path, root: &Path, reference: &Value) -> Result<Value> {
    let id = uuid(&reference["image"])?;
    let sha = reference["sha256"]
        .as_str()
        .filter(|s| store::valid_digest(s))
        .ok_or("image digest required")?;
    if !root.exists() {
        return Err("image store unavailable".into());
    }
    let selected = query(binary, root, "image-info", Some(Path::new(id)))?;
    if selected["sha256"] != sha {
        return Err("image reference changed".into());
    }
    Ok(selected)
}
pub(super) fn selected(service: &Path, environment: &str, reference: &Value) -> Result<Value> {
    let (runtime, manifest) = runtime(service, environment)?;
    let selected = image(
        &binary(&runtime, &manifest)?,
        &state_root(service, environment)?,
        reference,
    )?;
    if selected["approved"] != true {
        return Err("image requires host approval".into());
    }
    Ok(selected)
}
mod maintenance;
mod operations;
pub(super) use maintenance::inventory;
mod preview;
mod tasks;
pub(super) use operations::{control, list, status, submit};
pub(super) use preview::preview;
pub(super) use tasks::{ready, settle_task_branch};

#[cfg(test)]
mod tests;
