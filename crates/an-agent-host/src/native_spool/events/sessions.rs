//! Explicit foreground coordination over existing finite dispatches, not a scheduler.
use super::*;
mod execution;
mod observation;
pub use execution::run;
pub use observation::inspect;

fn read(path: &Path) -> Result<Value> {
    serde_json::from_slice(&regular(path, 4 * 1024 * 1024)?).map_err(Into::into)
}
fn open(workspace: &Path, name: &str) -> Result<(Space, PathBuf, Value)> {
    native::bundle::identifier(name)?;
    let space = Space::observe(workspace)?;
    let root = space.root.join("sessions").join(name);
    let record = read(&root.join("session.json"))?;
    if record["schema"] != "an-native-session-v1"
        || record["workspace"] != space.id
        || record["name"] != name
    {
        return Err("session identity mismatch".into());
    }
    uuid::Uuid::parse_str(text(&record, "id")?)?;
    Ok((space, root, record))
}
fn control(root: &Path) -> Result<Value> {
    let mut paths = fs::read_dir(root.join("controls"))?
        .map(|e| e.map(|v| v.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.sort();
    if paths.is_empty() || paths.len() > 128 {
        return Err("invalid session control history".into());
    }
    let mut prior = Value::Null;
    for (index, path) in paths.iter().enumerate() {
        if path.file_name().and_then(|s| s.to_str()) != Some(&format!("{index:03}.json")) {
            return Err("session control history gap".into());
        }
        let value = read(path)?;
        if value["generation"] != index
            || value["previous"] != prior
            || !matches!(
                value["mode"].as_str(),
                Some("open" | "paused" | "cancelled" | "closed")
            )
        {
            return Err("invalid session control transition".into());
        }
        prior = json!(hash(&value)?);
    }
    read(paths.last().ok_or("missing control")?)
}
fn threads(root: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = fs::read_dir(root.join("threads"))?
        .map(|e| e.map(|v| v.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.sort();
    if paths.len() > 32 {
        return Err("session thread limit exceeded".into());
    }
    Ok(paths)
}
pub fn create(workspace: &Path, name: &str) -> Result<Value> {
    native::bundle::identifier(name)?;
    let space = Space::open(workspace)?;
    let _lock = store::lock(&space.root)?;
    let parent = space.root.join("sessions");
    store::directory(&parent)?;
    if fs::read_dir(&parent)?.take(32).count() >= 32 {
        return Err("session limit reached".into());
    }
    let root = parent.join(name);
    fs::create_dir(&root)?;
    store::directory(&root.join("threads"))?;
    store::directory(&root.join("controls"))?;
    let record = json!({"schema":"an-native-session-v1","id":an_agent_core::det_seam::Entropy::os().uuid_v4(),
        "name":name,"workspace":space.id});
    write_json(&root.join("session.json"), &record)?;
    write_json(
        &root.join("controls/000.json"),
        &json!({"generation":0,"mode":"open","previous":null}),
    )?;
    fs::File::open(root.join("controls"))?.sync_all()?;
    fs::File::open(&root)?.sync_all()?;
    fs::File::open(&parent)?.sync_all()?;
    fs::File::open(&space.root)?.sync_all()?;
    Ok(record)
}
pub fn change(workspace: &Path, name: &str, mode: &str, approved: Option<&str>) -> Result<Value> {
    if !matches!(mode, "open" | "paused" | "cancelled" | "closed") {
        return Err("invalid session mode".into());
    }
    let (space, root, record) = open(workspace, name)?;
    let _lock = store::lock(&root)?;
    let prior = control(&root)?;
    if prior["mode"] == "closed" || (prior["mode"] == "cancelled" && mode != "closed") {
        return Err("session is permanently closed to new work".into());
    }
    let generation = prior["generation"].as_u64().ok_or("generation")? + 1;
    if generation >= 128 {
        return Err("session control limit reached".into());
    }
    if mode == "closed" {
        for path in threads(&root)? {
            observation::settled(&space, &record, &path)?;
        }
    }
    let plan = json!({"operation":"session-control","session":record,"prior":prior,"mode":mode,
        "generation":generation,"deletes_artifacts":false});
    let (digest, execute) = native::approval(&plan, approved)?;
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":digest,"executed":false}));
    }
    // Readers only see complete committed control records. A failed staging write
    // remains for inspection and prevents another control publication.
    let pending = root.join("control.pending");
    write_json(
        &pending,
        &json!({"generation":generation,"mode":mode,
        "previous":hash(&prior)?,"approval_sha256":digest}),
    )?;
    fs::rename(
        &pending,
        root.join("controls").join(format!("{generation:03}.json")),
    )?;
    fs::File::open(root.join("controls"))?.sync_all()?;
    fs::File::open(&root)?.sync_all()?;
    Ok(json!({"session":record,"mode":mode,"generation":generation,
        "cancellation_confirmed":false,"deletes_artifacts":false}))
}

#[cfg(test)]
mod tests;
