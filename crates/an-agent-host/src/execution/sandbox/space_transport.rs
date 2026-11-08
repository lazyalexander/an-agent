//! Bounded local SPACE client process; remote lifecycle remains backend evidence.
use super::{Execution, Result, execute_command};
use std::{path::Path, process::Command, sync::atomic::AtomicBool, time::Duration};

pub(crate) fn call(
    python: &Path,
    directory: &Path,
    step: &str,
    seconds: u64,
    cancelled: &AtomicBool,
) -> Result<Execution> {
    let mut command = Command::new(python);
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .arg(directory.join("space-client.py"))
        .arg(directory.join("space-profile.json"))
        .arg(directory)
        .arg(step);
    execute_command(
        command,
        directory,
        b"",
        Duration::from_secs(seconds + 10),
        cancelled,
        false,
    )
}
