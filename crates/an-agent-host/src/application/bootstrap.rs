//! Start the existing daemon for a new product workspace; never replace an owner.
use crate::execution::{Result, store};
use serde_json::{Value, json};
use std::{fs, path::Path, time::Duration};
pub(super) fn ensure(root: &Path) -> Result<()> {
    if !root.exists() {
        crate::daemon::init(root, 2)?;
    }
    store::no_links(root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let metadata = fs::metadata(root)?;
        if metadata.permissions().mode() & 0o077 != 0
            || metadata.uid() != unsafe { libc::geteuid() }
        {
            return Err("private owned service directory required".into());
        }
    }
    let config: Value = serde_json::from_slice(&store::read(&root.join("config.json"), 65536)?)?;
    if config["executable"] != json!(std::env::current_exe()?.canonicalize()?) {
        return Err("service executable selection changed".into());
    }
    if crate::daemon::request(root, &json!({"op":"show"})).is_ok() {
        return Ok(());
    }
    let log = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(root.join("application-daemon.log"))?;
    let mut child = crate::execution::sandbox::product_process::daemon(root, log)?;
    let deadline = an_agent_core::det_seam::Clock::monotonic_now() + Duration::from_secs(15);
    loop {
        if crate::daemon::request(root, &json!({"op":"show"})).is_ok() {
            return Ok(());
        }
        if child.try_wait()?.is_some() || an_agent_core::det_seam::Clock::monotonic_now() > deadline
        {
            return Err("original daemon did not become available".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
