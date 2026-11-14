//! Fixed trusted product-controller launches; workload execution remains supervised elsewhere.
use crate::execution::Result;
use std::{
    fs::File,
    path::Path,
    process::{Child, Command, Stdio},
};
fn host_child(root: &Path, log: File, task: Option<&str>) -> Result<Child> {
    let mut command = Command::new(std::env::current_exe()?);
    if let Some(id) = task {
        command.args(["app", "task-worker"]).arg(root).arg(id);
    } else {
        command.args(["daemon", "serve"]).arg(root);
    }
    command
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    Ok(command.spawn()?)
}
pub(crate) fn daemon(root: &Path, log: File) -> Result<Child> {
    host_child(root, log, None)
}
pub(crate) fn task(root: &Path, id: &str, log: File) -> Result<Child> {
    host_child(root, log, Some(id))
}
pub(crate) fn replace_controller(
    python: &Path,
    script: &Path,
    service: &Path,
    root: &Path,
) -> Result<serde_json::Value> {
    let mut command = Command::new(python);
    command
        .args(["-I", "-B"])
        .arg(script)
        .arg(std::env::current_exe()?)
        .arg(service)
        .arg(root);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(command.exec().into())
    }
    #[cfg(not(unix))]
    {
        let _ = command;
        Err("product workers require macOS".into())
    }
}
