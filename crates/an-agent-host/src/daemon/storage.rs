//! Private service configuration, bounded registries and durable launch fences.
use super::*;

pub fn init(root: &Path, concurrency: usize) -> Result<Value> {
    if !cfg!(target_os = "macos") || !(1..=8).contains(&concurrency) {
        return Err("macOS daemon concurrency must be between 1 and 8".into());
    }
    if root.exists() {
        return Err("daemon init requires a new private directory".into());
    }
    store::directory(root)?;
    let root = private(root)?;
    for name in ["instances", "jobs"] {
        store::directory(&root.join(name))?;
    }
    write(
        &root.join("config.json"),
        &Config {
            schema: "an-user-daemon-v1".into(),
            concurrency,
            executable: std::env::current_exe()?.canonicalize()?,
        },
    )?;
    Ok(json!({"root":root,"concurrency":concurrency,"installed":false}))
}

pub(super) fn private(root: &Path) -> Result<PathBuf> {
    store::no_links(root)?;
    let root = root.canonicalize()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let metadata = fs::metadata(&root)?;
        if unsafe { libc::geteuid() } == 0
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err("daemon directory must belong to this user with mode 0700".into());
        }
    }
    Ok(root)
}

pub(super) fn config(root: &Path) -> Result<Config> {
    private(root)?;
    let c: Config = serde_json::from_value(read(&root.join("config.json"))?)?;
    if c.schema != "an-user-daemon-v1"
        || !(1..=8).contains(&c.concurrency)
        || c.executable != std::env::current_exe()?.canonicalize()?
    {
        return Err("daemon configuration or trusted host executable changed".into());
    }
    Ok(c)
}

pub(super) fn entries(root: &Path, name: &str, limit: usize) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(root.join(name))? {
        let path = entry?.path();
        store::no_links(&path)?;
        paths.push(path);
        if paths.len() > limit {
            return Err("daemon registry bound exceeded; archive settled work explicitly".into());
        }
    }
    paths.sort();
    Ok(paths)
}

pub(super) fn instance(root: &Path, selected: &Path) -> Result<Value> {
    store::no_links(selected)?;
    let selected = selected.canonicalize()?;
    let current = spool::show(&selected)?;
    let id = current["instance"].as_str().ok_or("spool identity")?;
    let registration = read(&root.join("instances").join(format!("{id}.json")))?;
    if registration["path"] != json!(selected) || registration["release"] != current["release"] {
        return Err("instance is not registered with this daemon".into());
    }
    Ok(current)
}

pub(super) fn register(root: &Path, selected: &Path) -> Result<Value> {
    if entries(root, "instances", 128)?.len() >= 128 {
        return Err("instance registry full".into());
    }
    store::no_links(selected)?;
    let selected = selected.canonicalize()?;
    if selected.starts_with(root) || root.starts_with(&selected) {
        return Err("service and spool stores must be separate".into());
    }
    let value = spool::show(&selected)?;
    let id = value["instance"].as_str().ok_or("spool identity")?;
    let record = json!({"path":selected,"instance":id,"release":value["release"]});
    let path = root.join("instances").join(format!("{id}.json"));
    if path.exists() {
        if read(&path)? != record {
            return Err("instance registration conflict".into());
        }
    } else {
        write(&path, &record)?;
    }
    Ok(record)
}

pub(super) fn job(root: &Path, id: &str) -> Result<(PathBuf, Job)> {
    if uuid::Uuid::parse_str(id)?.to_string() != id {
        return Err("invalid daemon task ID".into());
    }
    let dir = root.join("jobs").join(id);
    store::no_links(&dir)?;
    let job = serde_json::from_value(read(&dir.join("request.json"))?)?;
    Ok((dir, job))
}

pub fn launchd(root: &Path, output: &Path) -> Result<Value> {
    let root = private(root)?;
    let c = config(&root)?;
    let xml = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    };
    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>local.an-agent.daemon</string>
<key>ProgramArguments</key><array><string>{}</string><string>daemon</string><string>serve</string><string>{}</string></array>
<key>RunAtLoad</key><true/><key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>
<key>ThrottleInterval</key><integer>10</integer>
<key>StandardOutPath</key><string>{}/service.log</string>
<key>StandardErrorPath</key><string>{}/service-error.log</string>
</dict></plist>
"#,
        xml(&c.executable.to_string_lossy()),
        xml(&root.to_string_lossy()),
        xml(&root.to_string_lossy()),
        xml(&root.to_string_lossy())
    );
    store::no_links(output)?;
    store::write_new(output, content.as_bytes())?;
    Ok(json!({"plist":output,"installed":false,"scope":"current logged-in user"}))
}
