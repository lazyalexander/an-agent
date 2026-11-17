//! Data-only backend selection. Provenance and resource requests never grant execution.
use super::manifest::Manifest;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const METADATA: [&str; 2] = ["backend", "container_provenance"];
pub const COMPONENTS: [&str; 9] = [
    "container",
    "container_apiserver",
    "container_runtime_linux",
    "container_network_vmnet",
    "container_images",
    "container_kernel",
    "container_init",
    "container_image",
    "container_swift_lock",
];

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Declaration {
    pub schema: String,
    pub backend: String,
    pub image: String,
    pub cpus: u32,
    pub memory_mib: u64,
    pub storage_bytes: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Provenance {
    schema: String,
    repository: String,
    commit: String,
    swift_lock_sha256: String,
    files: BTreeMap<String, String>,
}

fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn is_metadata(manifest: &Manifest, path: &str) -> bool {
    METADATA
        .iter()
        .any(|key| manifest.components.get(*key).is_some_and(|p| p == path))
}

fn data(
    manifest: &Manifest,
    key: &str,
    read: &mut impl FnMut(&str) -> Result<Vec<u8>, String>,
) -> Result<Vec<u8>, String> {
    let path = manifest
        .components
        .get(key)
        .ok_or("missing Container data component")?;
    let file = manifest
        .files
        .get(path)
        .ok_or("missing Container data payload")?;
    if file.executable || file.bytes > 65536 {
        return Err("Container declarations must be bounded data".into());
    }
    let bytes = read(path)?;
    if bytes.len() as u64 != file.bytes || format!("{:x}", Sha256::digest(&bytes)) != file.sha256 {
        return Err("Container declaration changed".into());
    }
    Ok(bytes)
}

pub fn validate(
    manifest: &Manifest,
    mut read: impl FnMut(&str) -> Result<Vec<u8>, String>,
) -> Result<Option<Declaration>, String> {
    if !manifest.components.contains_key("backend") {
        if manifest
            .components
            .keys()
            .any(|key| key == "container" || key.starts_with("container_"))
        {
            return Err("Container payload requires an explicit backend declaration".into());
        }
        return Ok(None);
    }
    let declaration: Declaration = serde_json::from_slice(&data(manifest, "backend", &mut read)?)
        .map_err(|e| e.to_string())?;
    if declaration.schema != "an-container-backend-v1"
        || declaration.backend != "apple-container"
        || !(1..=8).contains(&declaration.cpus)
        || !(256..=16384).contains(&declaration.memory_mib)
        || !(256 * 1024 * 1024..=64 * 1024 * 1024 * 1024).contains(&declaration.storage_bytes)
        || declaration.image.len() > 1024
        || !declaration
            .image
            .rsplit_once("@sha256:")
            .is_some_and(|(name, digest)| {
                !name.is_empty()
                    && !name.starts_with('-')
                    && hex(digest, 64)
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"/._-:".contains(&b))
            })
    {
        return Err("unsupported Container backend or resource request".into());
    }
    // Mixed image APIs cannot silently route to the legacy lifecycle.
    if ["cocoon", "runner", "image_api", "image_maintenance_api"]
        .iter()
        .any(|key| manifest.components.contains_key(*key))
    {
        return Err("mixed Container and Cocoon runtime declarations".into());
    }
    let provenance: Provenance =
        serde_json::from_slice(&data(manifest, "container_provenance", &mut read)?)
            .map_err(|e| e.to_string())?;
    if provenance.schema != "an-container-provenance-v1"
        || provenance.repository != "https://github.com/lazyalexander/container.git"
        || !hex(&provenance.commit, 40)
        || !hex(&provenance.swift_lock_sha256, 64)
        || provenance.files.is_empty()
        || provenance.files.len() > 16384
    {
        return Err("invalid Container source provenance".into());
    }
    for (path, hash) in &provenance.files {
        if is_metadata(manifest, path)
            || !manifest
                .files
                .get(path)
                .is_some_and(|file| file.sha256 == *hash)
        {
            return Err("Container provenance payload mismatch".into());
        }
    }
    for (index, key) in COMPONENTS.iter().enumerate() {
        let path = manifest
            .components
            .get(*key)
            .ok_or("missing offline Container component")?;
        let file = manifest
            .files
            .get(path)
            .ok_or("missing offline Container payload")?;
        if provenance.files.get(path) != Some(&file.sha256)
            || file.bytes == 0
            || (index < 5 && !file.executable)
            || (index >= 5 && file.executable)
        {
            return Err("Container component provenance or mode mismatch".into());
        }
    }
    if manifest.files[&manifest.components["container_swift_lock"]].sha256
        != provenance.swift_lock_sha256
    {
        return Err("Container Swift dependency lock mismatch".into());
    }
    Ok(Some(declaration))
}

#[cfg(test)]
mod tests;
