//! Private instance metadata and committed revision traversal.
use super::*;
use std::path::PathBuf;

pub(super) fn root_lock(root: &Path) -> Result<(PathBuf, store::Lock)> {
    store::no_links(root)?;
    let root = root.canonicalize()?;
    let lock = store::lock(&root)?;
    Ok((root, lock))
}
pub(super) fn instance_path(root: &Path, id: &str) -> Result<PathBuf> {
    if uuid::Uuid::parse_str(id)?.to_string() != id {
        return Err("instance must be a canonical UUID".into());
    }
    let path = root.join("instances").join(id);
    store::no_links(&path)?;
    Ok(path)
}
pub(super) fn catalog(path: &Path) -> PathBuf {
    path.join("catalog")
}
pub(super) fn head(path: &Path) -> Result<String> {
    let head = String::from_utf8(regular(&path.join("HEAD"), 64)?)?;
    if !store::valid_digest(&head) {
        return Err("invalid instance head".into());
    }
    Ok(head)
}
pub(super) fn release(path: &Path, id: &str) -> Result<String> {
    let metadata: Value = serde_json::from_slice(&regular(&path.join("instance.json"), 4096)?)?;
    if metadata["schema"] != "an-instance-v1" || metadata["instance"] != id {
        return Err("instance metadata mismatch".into());
    }
    Ok(metadata["release"]
        .as_str()
        .filter(|s| store::valid_digest(s))
        .ok_or("invalid instance release")?
        .into())
}
pub(super) fn check_release(root: &Path, package: &str) -> Result<()> {
    if !store::valid_digest(package) {
        return Err("invalid instance release".into());
    }
    let path = root.join("releases").join(package);
    if execution::check(&path)?["release"] != package {
        return Err("release mismatch".into());
    }
    let yaml = regular(&path.join("spool.yaml"), 65536)?;
    let spec = an_agent_spool::spool::parse(std::str::from_utf8(&yaml)?)?;
    if spec.config.get("profile").and_then(Value::as_str) != Some("file-kit-v1") {
        return Err("instance requires the native file-kit profile".into());
    }
    Ok(())
}
pub(super) fn read_bundle(path: &Path, revision: &str) -> Result<(Bundle, Value)> {
    let (bytes, binding) = artifact_versions::read(&catalog(path), revision)?;
    Ok((Bundle::parse(&bytes)?, binding))
}
pub(super) fn committed(path: &Path, current: &str, selected: &str) -> Result<()> {
    if !store::valid_digest(selected) {
        return Err("invalid state revision".into());
    }
    let mut cursor = current.to_owned();
    // Bounded initial profile: do not silently perform unbounded history scans.
    for _ in 0..1024 {
        let (_, binding) = read_bundle(path, &cursor)?;
        if cursor == selected {
            return Ok(());
        }
        let Some(parent) = binding["parent"].as_str() else {
            break;
        };
        cursor = parent.into();
    }
    Err("revision is not in bounded committed instance history".into())
}
pub(super) fn record(path: &Path, digest: &str, name: &str, value: &Value) -> Result<()> {
    let journals = path.join("commits");
    store::directory(&journals)?;
    let journal = journals.join(digest);
    store::directory(&journal)?;
    store::write_new(&journal.join(name), &serde_json::to_vec_pretty(value)?)
}
