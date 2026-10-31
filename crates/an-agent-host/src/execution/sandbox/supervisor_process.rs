//! Process detachment for a finite, operator-approved native supervisor.
use super::Result;
use std::{
    fs::OpenOptions,
    path::Path,
    process::{Child, Command, Stdio},
};

pub fn spawn(
    executable: &Path,
    root: &Path,
    name: &str,
    hash: &str,
    directory: &Path,
) -> Result<Child> {
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
        .args(["spool-supervisor", "worker"])
        .arg(root)
        .arg(name)
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
