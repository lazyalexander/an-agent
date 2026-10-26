//! Instance installation, bounded kit edits and explicit unloading.
use super::*;

pub fn load(package: &Path, destination: &Path) -> Result<Value> {
    let bundle = bundle::read(package)?;
    store::no_links(destination)?;
    if destination.exists() {
        return Err("load requires a new instance directory".into());
    }
    let root = destination
        .parent()
        .ok_or("missing parent")?
        .canonicalize()?
        .join(destination.file_name().ok_or("missing name")?);
    if root.starts_with(package.canonicalize()?) {
        return Err("instance must not modify its source release directory".into());
    }
    // Exclusive directory ownership; a partial load is never implicitly resumed.
    let mut directory = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        directory.mode(0o700);
    }
    directory.create(&root)?;
    for name in ["release", "release/silk", "release/kit", "kit", "runs"] {
        store::directory(&root.join(name))?;
    }
    for (path, bytes) in &bundle.payload {
        let target = root.join("release").join(path);
        store::write_new(&target, bytes)?;
        readonly(&target)?;
        if path.starts_with("kit/") {
            store::write_new(&root.join(path), bytes)?;
        }
    }
    store::write_new(&root.join("release").join(bundle::FILE), &bundle.bytes)?;
    readonly(&root.join("release").join(bundle::FILE))?;
    let instance = an_agent_core::det_seam::Entropy::os().uuid_v4().to_string();
    let record = json!({"schema":"an-native-spool-instance-v1","instance":instance,
        "release":bundle.release,"name":bundle.manifest.name,"version":bundle.manifest.version,
        "execution_granted":false,"profile":"macos-reviewed-native-spool-v1"});
    write_json(&root.join("instance.json"), &record)?;
    show(&root)
}

pub fn show(path: &Path) -> Result<Value> {
    let root = location(path)?;
    let (mut record, bundle) = state(&root)?;
    record["loaded"] = json!(!root.join("unloaded.json").exists());
    record["operation_locked"] = json!(root.join("host.lock").exists());
    record["capabilities"] = json!({"start":cfg!(target_os="macos"),"cancel":"running CLI SIGINT/SIGTERM", "pause":false,"suspend":false,"weaver":false});
    let mut kits = serde_json::Map::new();
    for path in bundle.payload.keys().filter(|p| p.starts_with("kit/")) {
        kits.insert(
            path.clone(),
            json!(store::digest(&regular(&root.join(path), 1024 * 1024)?)),
        );
    }
    record["kits"] = json!(kits);
    let mut runs = Vec::new();
    for entry in fs::read_dir(root.join("runs"))? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("invalid run")?;
        let status = if path.join("reclaimed.json").exists() {
            "reclaimed"
        } else if path.join("result.json").exists() {
            "result-recorded"
        } else if path.join("job").exists() {
            "running-or-unconfirmed"
        } else {
            "prepared"
        };
        runs.push(json!({"run":name,"observation":status}));
    }
    runs.sort_by_key(|r| r["run"].as_str().unwrap_or_default().to_owned());
    record["runs"] = json!(runs);
    Ok(record)
}

/// Host/operator editing route; never accepts a silk path or executes the edit.
pub fn update_kit(path: &Path, name: &str, source: &Path, approved: Option<&str>) -> Result<Value> {
    bundle::identifier(name)?;
    let root = location(path)?;
    let _lock = store::lock(&root)?;
    let (instance, bundle) = active(&root)?;
    let selected = format!("kit/{name}");
    if !bundle.payload.contains_key(&selected) {
        return Err("not a declared kit working copy".into());
    }
    let bytes = regular(source, 1024 * 1024)?;
    let target = root.join(&selected);
    let before = regular(&target, 1024 * 1024)?;
    let plan = json!({"instance":instance["instance"],"release":bundle.release,"kit":selected,
        "before":store::digest(&before),"after":store::digest(&bytes),"execution_granted":false});
    let (hash, execute) = approval(&plan, approved)?;
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":hash,"executed":false}));
    }
    let pending = root.join(format!(
        "{}.pending",
        an_agent_core::det_seam::Entropy::os().uuid_v4()
    ));
    store::write_new(&pending, &bytes)?;
    fs::rename(&pending, &target)?;
    write_json(
        &root.join(format!(
            "edit-{}.json",
            an_agent_core::det_seam::Entropy::os().uuid_v4()
        )),
        &plan,
    )?;
    Ok(json!({"status":"succeeded","edit":plan}))
}

pub fn unload(path: &Path, approved: Option<&str>) -> Result<Value> {
    let root = location(path)?;
    let _lock = store::lock(&root)?;
    let (instance, _) = active(&root)?;
    let mut runs = Vec::new();
    for entry in fs::read_dir(root.join("runs"))? {
        let path = entry?.path();
        if path.join("job").exists() {
            let result = super::run::settled(
                &root,
                path.file_name()
                    .and_then(|s| s.to_str())
                    .ok_or("invalid run")?,
            )?;
            runs.push(result);
        }
    }
    runs.sort_by_key(|r| r["run"].as_str().unwrap_or_default().to_owned());
    let plan = json!({"instance":instance["instance"],"release":instance["release"],"runs":runs,
        "preserve":"products, snapshots and receipts; reclaim each settled run explicitly"});
    let (hash, execute) = approval(&plan, approved)?;
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":hash,"executed":false}));
    }
    write_json(&root.join("unloaded.json"), &plan)?;
    Ok(json!({"status":"succeeded","loaded":false,"instance":instance["instance"]}))
}
