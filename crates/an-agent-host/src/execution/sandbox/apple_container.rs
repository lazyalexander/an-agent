//! Kit: bounded local Apple CLI process; guest termination needs runtime evidence.
use super::{Execution, Result, execute_command};
use std::{path::Path, process::Command, sync::atomic::AtomicBool, time::Duration};

pub(crate) fn call(
    cli: &Path,
    directory: &Path,
    args: &[&str],
    input: &[u8],
    seconds: u64,
) -> Result<Execution> {
    let mut command = Command::new(cli);
    command.env_clear().env("PATH", "/usr/bin:/bin").args(args);
    execute_command(
        command,
        directory,
        input,
        Duration::from_secs(seconds),
        &AtomicBool::new(false),
        false,
    )
}
