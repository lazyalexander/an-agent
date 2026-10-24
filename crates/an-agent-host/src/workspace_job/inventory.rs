//! Bounded, no-follow scratch inventories and exact, approved reclamation.
use crate::{
    execution::{Result, store},
    file_job::regular,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

pub(super) fn relative(path: &str) -> Result<()> {
    if path.is_empty()
        || path.len() > 1024
        || path.contains(['\\', ':'])
        || path
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
        || path.chars().any(char::is_control)
    {
        return Err("expected a portable relative file path".into());
    }
    Ok(())
}

pub(super) fn scan(root: &Path) -> Result<Value> {
    let mut entries = BTreeMap::new();
    let mut total = 0usize;
    visit(root, root, &mut entries, &mut total, 0)?;
    Ok(json!(entries))
}

fn visit(
    root: &Path,
    dir: &Path,
    entries: &mut BTreeMap<String, Value>,
    total: &mut usize,
    depth: usize,
) -> Result<()> {
    if depth > 32 {
        return Err("scratch directory depth exceeds limit".into());
    }
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let name = path
            .strip_prefix(root)?
            .to_str()
            .ok_or("non-UTF8 scratch path")?
            .to_owned();
        relative(&name)?;
        if entries.len() >= 2000 {
            return Err("scratch inventory exceeds 2000 entries".into());
        }
        let meta = fs::symlink_metadata(&path)?;
        let value = if meta.is_symlink() {
            json!({"kind":"symlink","target":fs::read_link(&path)?.to_str().ok_or("non-UTF8 link")?})
        } else if meta.is_dir() {
            json!({"kind":"directory"})
        } else if meta.is_file() {
            let bytes = regular(&path, 64 * 1024 * 1024)?;
            *total = total.checked_add(bytes.len()).ok_or("inventory overflow")?;
            if *total > 128 * 1024 * 1024 {
                return Err("scratch exceeds 128 MiB".into());
            }
            json!({"kind":"file","bytes":bytes.len(),"sha256":store::digest(&bytes)})
        } else {
            return Err("special scratch file refused".into());
        };
        entries.insert(name, value);
        if meta.is_dir() && !meta.is_symlink() {
            visit(root, &path, entries, total, depth + 1)?;
        }
    }
    Ok(())
}

pub(super) fn remove(root: &Path, inventory: &Value) -> Result<()> {
    let mut entries: Vec<_> = inventory
        .as_object()
        .ok_or("invalid inventory")?
        .iter()
        .collect();
    entries.sort_by_key(|(name, _)| std::cmp::Reverse(name.split('/').count()));
    for (name, item) in entries {
        let path = root.join(name);
        if item["kind"] == "directory" {
            fs::remove_dir(path)?;
        } else {
            fs::remove_file(path)?;
        }
    }
    fs::remove_dir(root)?;
    Ok(())
}
