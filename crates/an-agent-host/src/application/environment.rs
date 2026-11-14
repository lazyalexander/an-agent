//! Offline verification and atomic installation; importing bytes never executes them.
use super::manifest::Manifest;
use crate::execution::{Result, store};
use flate2::read::GzDecoder;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};

fn hash_stream(
    input: &mut impl Read,
    mut output: Option<&mut fs::File>,
    size: u64,
) -> Result<String> {
    let mut hash = Sha256::new();
    let mut remaining = size;
    let mut bytes = [0u8; 65536];
    while remaining > 0 {
        let limit = (remaining.min(bytes.len() as u64)) as usize;
        let count = input.read(&mut bytes[..limit])?;
        if count == 0 {
            return Err("truncated runtime payload".into());
        }
        hash.update(&bytes[..count]);
        if let Some(file) = output.as_mut() {
            if bytes[..count].iter().all(|b| *b == 0) {
                file.seek(SeekFrom::Current(count as i64))?;
            } else {
                file.write_all(&bytes[..count])?;
            }
        }
        remaining -= count as u64;
    }
    Ok(format!("{:x}", hash.finalize()))
}
fn inspect(archive: &Path, destination: Option<&Path>) -> Result<Manifest> {
    store::no_links(archive)?;
    let mut archive = tar::Archive::new(GzDecoder::new(fs::File::open(archive)?));
    let mut entries = archive.entries()?.raw(true);
    let mut first = entries.next().ok_or("runtime manifest missing")??;
    if first.path_bytes().as_ref() != b"runtime.json"
        || !first.header().entry_type().is_file()
        || first.size() > 2 * 1024 * 1024
    {
        return Err("runtime manifest must be the first bounded regular entry".into());
    }
    let mut bytes = Vec::new();
    first.read_to_end(&mut bytes)?;
    let manifest: Manifest = serde_json::from_slice(&bytes)?;
    manifest
        .validate()
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    drop(first);
    for (name, declared) in &manifest.files {
        let mut entry = entries.next().ok_or("runtime payload missing")??;
        if entry.path_bytes().as_ref() != name.as_bytes()
            || !entry.header().entry_type().is_file()
            || entry.size() != declared.bytes
        {
            return Err("runtime payload order, type or size mismatch".into());
        }
        let mut output = if let Some(root) = destination {
            let path = root.join(name);
            let parent = path.parent().ok_or("runtime parent")?;
            fs::create_dir_all(parent)?;
            store::no_links(&path)?;
            Some(
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)?,
            )
        } else {
            None
        };
        if hash_stream(&mut entry, output.as_mut(), declared.bytes)? != declared.sha256 {
            return Err("runtime payload digest mismatch".into());
        }
        if let Some(file) = output {
            file.set_len(declared.bytes)?;
            file.sync_all()?;
        }
        #[cfg(unix)]
        if let Some(root) = destination {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                root.join(name),
                fs::Permissions::from_mode(if declared.executable { 0o500 } else { 0o400 }),
            )?;
        }
    }
    if entries.next().is_some() {
        return Err("undeclared runtime payload".into());
    }
    Ok(manifest)
}
pub fn preview(archive: &Path) -> Result<Value> {
    let manifest = inspect(archive, None)?;
    let bytes = serde_json::to_vec(&manifest)?;
    Ok(json!({"manifest":manifest,"approval_sha256":store::digest(&bytes),"executed":false}))
}
pub fn import(root: &Path, archive: &Path, approval: &str) -> Result<Value> {
    if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        return Err("runtime platform unsupported".into());
    }
    store::directory(root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let metadata = fs::metadata(root)?;
        if metadata.permissions().mode() & 0o077 != 0
            || metadata.uid() != unsafe { libc::geteuid() }
        {
            return Err("environment store must be private and owned by this user".into());
        }
    }
    let preview = preview(archive)?;
    if preview["approval_sha256"] != approval {
        return Err("runtime approval mismatch".into());
    }
    let target = root.join(approval);
    if target.exists() {
        let installed = store::read(&target.join("runtime.json"), 2 * 1024 * 1024)?;
        if store::digest(&installed) != approval {
            return Err("installed runtime identity mismatch".into());
        }
        validate_installed(&target)?;
        return Ok(json!({"installed":true,"id":approval,"reused":true}));
    }
    let temporary = root.join(format!(
        ".pending-{}",
        an_agent_core::det_seam::Entropy::os().uuid_v4()
    ));
    store::directory(&temporary)?;
    let result = (|| -> Result<Value> {
        let manifest = inspect(archive, Some(&temporary))?;
        let bytes = serde_json::to_vec(&manifest)?;
        if store::digest(&bytes) != approval {
            return Err("runtime changed after approval".into());
        }
        store::write_new(&temporary.join("runtime.json"), &bytes)?;
        fs::File::open(&temporary)?.sync_all()?;
        fs::rename(&temporary, &target)?;
        fs::File::open(root)?.sync_all()?;
        Ok(json!({"installed":true,"id":approval,"reused":false}))
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&temporary);
    }
    result
}

pub fn validate_installed(root: &Path) -> Result<Value> {
    let bytes = store::read(&root.join("runtime.json"), 2 * 1024 * 1024)?;
    let manifest: Manifest = serde_json::from_slice(&bytes)?;
    manifest
        .validate()
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    for (name, file) in &manifest.files {
        let path = root.join(name);
        store::no_links(&path)?;
        let mut input = fs::File::open(path)?;
        if input.metadata()?.len() != file.bytes
            || hash_stream(&mut input, None, file.bytes)? != file.sha256
        {
            return Err("installed environment changed".into());
        }
    }
    Ok(json!({"id":store::digest(&bytes),"manifest":manifest,"verified":true}))
}
#[cfg(test)]
mod tests;
