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

/// The original page process retains wait ownership of this exact child.
pub(crate) fn reap(mut child: Child) -> Result<()> {
    std::thread::Builder::new()
        .name("product-owner-reaper".into())
        .spawn(move || child.wait())?;
    Ok(())
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

/// Signal only a still-matching recorded process; never substitute another PID.
pub(crate) fn stop_control(identity: &serde_json::Value) -> Result<bool> {
    let pid = identity["pid"]
        .as_u64()
        .filter(|p| *p > 1 && *p <= i32::MAX as u64)
        .ok_or("invalid control process identity")? as u32;
    if super::service_process::exited(pid) {
        return Ok(true);
    }
    if super::service_process::identity(pid) != *identity {
        return Err("control process identity changed".into());
    }
    #[cfg(unix)]
    if unsafe { libc::kill(pid as i32, libc::SIGTERM) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let deadline =
        an_agent_core::det_seam::Clock::monotonic_now() + std::time::Duration::from_secs(10);
    while an_agent_core::det_seam::Clock::monotonic_now() < deadline {
        if super::service_process::exited(pid) {
            return Ok(true);
        }
        // Exit can make BSD birth metadata unavailable before PID removal.
        // Observe only after signaling; never signal a replacement process.
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    Ok(false)
}

#[cfg(all(test, target_os = "macos"))]
pub(crate) fn test_control() -> Result<Child> {
    Ok(Command::new("/bin/sleep").arg("60").spawn()?)
}

/// Fixed finite image commands, separate from the model's work-lane transport.
pub(crate) fn image_query(
    binary: &Path,
    root: &Path,
    command: &str,
    argument: Option<&Path>,
) -> Result<serde_json::Value> {
    if !matches!(
        command,
        "image-maintenance-info"
            | "image-inventory"
            | "image-init"
            | "image-info"
            | "disk-info"
            | "image-status"
            | "image-cancel"
            | "image-reconcile"
    ) {
        return Err("unsupported image query".into());
    }
    let mut child = Command::new(binary);
    child
        .arg("host")
        .arg(root)
        .arg(command)
        .stdin(Stdio::null());
    if let Some(argument) = argument {
        child.arg(argument);
    }
    let output = child.output()?;
    if !output.status.success() {
        return Err("Cocoon image query failed; inspect the original private record".into());
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}
pub(crate) fn image_submit(binary: &Path, root: &Path, request: &Path, log: File) -> Result<Child> {
    Ok(Command::new(binary)
        .arg("host")
        .arg(root)
        .arg("image-request")
        .arg(request)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .spawn()?)
}
