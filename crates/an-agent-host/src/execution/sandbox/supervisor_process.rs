//! Process detachment for a finite, operator-approved native supervisor.
use super::Result;
use std::{
    fs::OpenOptions,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

pub fn host_executable(path: &Path) -> Result<PathBuf> {
    let installed = std::env::current_exe()?.canonicalize()?;
    if path.canonicalize()? != installed {
        return Err("independent supervision requires the current trusted host executable".into());
    }
    Ok(installed)
}

pub fn spawn(
    executable: &Path,
    root: &Path,
    name: &str,
    hash: &str,
    directory: &Path,
) -> Result<Child> {
    spawn_worker(executable, root, directory, "worker", name, None, hash)
}

pub fn spawn_coordinator(
    executable: &Path,
    root: &Path,
    name: &str,
    thread: Option<&str>,
    hash: &str,
    directory: &Path,
) -> Result<Child> {
    spawn_worker(
        executable,
        root,
        directory,
        "coordinator-worker",
        name,
        thread,
        hash,
    )
}

fn spawn_worker(
    executable: &Path,
    root: &Path,
    directory: &Path,
    verb: &str,
    name: &str,
    thread: Option<&str>,
    hash: &str,
) -> Result<Child> {
    host_executable(executable)?;
    let stdout = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join("stdout.log"))?;
    let stderr = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join("stderr.log"))?;
    let mut command = Command::new(executable);
    command
        .args(["spool-supervisor", verb])
        .arg(root)
        .arg(name)
        .args(thread)
        .arg(hash)
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr);
    detach(&mut command)?;
    Ok(command.spawn()?)
}

#[cfg(unix)]
fn detach(command: &mut Command) -> Result<()> {
    use std::os::unix::process::CommandExt;
    // SAFETY: setsid is the only child-side operation before exec. The worker
    // owns a new session and inherits no terminal or client-owned pipe.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    Ok(())
}
#[cfg(not(unix))]
fn detach(_: &mut Command) -> Result<()> {
    Err("independent supervisor unsupported by this backend".into())
}
