//! A deliberately narrow native backend. No unsupported-platform fallback.
use std::io::{Read, Write};
use std::path::Path;
use std::process::Stdio;
mod process;
pub(crate) mod workspace;
use process::OwnedChild;

#[cfg(target_os = "linux")]
pub(crate) mod linux;
#[cfg(any(not(target_os = "linux"), test))]
pub(crate) mod macos;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use super::Result;

pub fn plan(executable: &Path) -> Result<serde_json::Value> {
    #[cfg(target_os = "linux")]
    {
        Ok(linux::plan(executable))
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(
            serde_json::json!({"backend":"macos-seatbelt-stdio-v1","command":["/usr/bin/sandbox-exec","-p",macos::profile(executable)?,executable],"supported":cfg!(target_os="macos")}),
        )
    }
}

fn command(executable: &Path) -> Result<std::process::Command> {
    #[cfg(target_os = "linux")]
    {
        linux::command(executable)
    }
    #[cfg(target_os = "macos")]
    {
        macos::command(executable)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = executable;
        Err("unsupported native sandbox".into())
    }
}

pub const INPUT_LIMIT: usize = 8192;
const OUTPUT_LIMIT: usize = 65536;

pub struct Execution {
    pub status: &'static str,
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub pid: Option<u32>,
    pub reaped: bool,
    pub group_cleanup_required: bool,
}

/// The standard process API reports spawn and pre-exec failures together.
/// Do not claim a child was launched or attribute an errno to a specific policy.
#[derive(Debug, thiserror::Error)]
#[error("spawn or pre-exec refused: {0}")]
struct LaunchError(#[source] std::io::Error);

pub(crate) fn refusal_evidence(error: &(dyn std::error::Error + 'static)) -> serde_json::Value {
    let mut current = Some(error);
    let mut os_error = None;
    let mut stage = "sandbox-setup";
    while let Some(cause) = current {
        if cause.is::<LaunchError>() && stage != "executable-publication" {
            stage = "spawn-or-pre-exec";
        }
        if cause.is::<super::publication::PublicationError>() {
            stage = "executable-publication";
        }
        if let Some(cause) = cause.downcast_ref::<std::io::Error>() {
            os_error = Some(cause);
        }
        current = cause.source();
    }
    serde_json::json!({"stage":stage,"message":error.to_string(),
        "os_code":os_error.and_then(std::io::Error::raw_os_error),
        "io_kind":os_error.map(|error| format!("{:?}", error.kind())),
        "retry_attempted":false})
}

fn reader(mut pipe: impl Read, exceeded: Arc<AtomicBool>) -> std::io::Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let count = match pipe.read(&mut chunk) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if count == 0 {
            break;
        }
        let available = OUTPUT_LIMIT.saturating_sub(output.len());
        output.extend_from_slice(&chunk[..count.min(available)]);
        if count > available {
            exceeded.store(true, Ordering::Relaxed);
        }
    }
    Ok(output)
}

pub fn execute(
    executable: &Path,
    cwd: &Path,
    input: &[u8],
    timeout: Duration,
    cancelled: &AtomicBool,
) -> Result<Execution> {
    if input.len() > INPUT_LIMIT {
        return Err("input exceeds 8192 bytes".into());
    }
    let executable = executable.canonicalize()?;
    if cancelled.load(Ordering::Relaxed) {
        return Ok(Execution {
            status: "cancelled",
            code: None,
            stdout: vec![],
            stderr: vec![],
            pid: None,
            reaped: false,
            group_cleanup_required: false,
        });
    }
    let mut launch = command(&executable)?;
    launch.env_clear();
    execute_command(launch, cwd, input, timeout, cancelled, false)
}

/// Host-built commands only. Policy and environment must already be installed.
pub(crate) fn execute_command(
    mut launch: std::process::Command,
    cwd: &Path,
    input: &[u8],
    timeout: Duration,
    cancelled: &AtomicBool,
    group: bool,
) -> Result<Execution> {
    if cancelled.load(Ordering::Relaxed) {
        return Ok(Execution {
            status: "cancelled",
            code: None,
            stdout: vec![],
            stderr: vec![],
            pid: None,
            reaped: false,
            group_cleanup_required: false,
        });
    }
    launch
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // The owner lives INSIDE the scope: kill/reap runs before implicit worker
    // joins on early return or unwind. No detached pipe workers survive a return.
    let io_failed = AtomicBool::new(false);
    #[allow(
        clippy::disallowed_methods,
        reason = "OS process wall-time enforcement"
    )]
    let start = Instant::now();
    thread::scope(|scope| {
        #[cfg(target_os = "macos")]
        let mut child = if group {
            OwnedChild::group(launch.spawn().map_err(LaunchError)?)
        } else {
            OwnedChild::new(launch.spawn().map_err(LaunchError)?)
        };
        #[cfg(not(target_os = "macos"))]
        let mut child = {
            if group {
                return Err("process-group jobs unsupported on this platform".into());
            }
            OwnedChild::new(launch.spawn().map_err(LaunchError)?)
        };
        let pid = Some(child.child.id());
        let result = (|| -> Result<Execution> {
            let mut stdin = child.child.stdin.take().ok_or("missing stdin")?;
            let stdout = child.child.stdout.take().ok_or("missing stdout")?;
            let stderr = child.child.stderr.take().ok_or("missing stderr")?;
            let exceeded = Arc::new(AtomicBool::new(false));
            let failed = &io_failed;
            let stdout_limit = Arc::clone(&exceeded);
            let stderr_limit = Arc::clone(&exceeded);
            let input_thread = thread::Builder::new()
                .name("kit-stdin".into())
                .spawn_scoped(scope, move || {
                    let result = stdin.write_all(input);
                    if result.is_err() {
                        failed.store(true, Ordering::Relaxed);
                    }
                    result
                })?;
            let out_thread = thread::Builder::new()
                .name("kit-stdout".into())
                .spawn_scoped(scope, move || {
                    let result = reader(stdout, stdout_limit);
                    if result.is_err() {
                        failed.store(true, Ordering::Relaxed);
                    }
                    result
                })?;
            let err_thread = thread::Builder::new()
                .name("kit-stderr".into())
                .spawn_scoped(scope, move || {
                    let result = reader(stderr, stderr_limit);
                    if result.is_err() {
                        failed.store(true, Ordering::Relaxed);
                    }
                    result
                })?;
            let mut reason = None;
            let status = loop {
                // First observed stop cause wins; later cleanup races cannot
                // turn cancellation into timeout or output-limit failure.
                if reason.is_none() {
                    reason = if cancelled.load(Ordering::Relaxed) {
                        Some("cancelled")
                    } else if exceeded.load(Ordering::Relaxed) {
                        Some("output_limit")
                    } else if io_failed.load(Ordering::Relaxed) {
                        Some("failed")
                    } else if start.elapsed() >= timeout {
                        Some("timed_out")
                    } else {
                        None
                    };
                }
                if let Some(status) = child.poll()? {
                    break status;
                }
                if reason.is_some() {
                    break child.terminate()?;
                }
                thread::sleep(Duration::from_millis(10));
            };
            // Join all workers before inspecting errors; an error in one stream
            // must not detach the other stream or an input writer.
            let input_result = input_thread.join();
            let out_result = out_thread.join();
            let err_result = err_thread.join();
            let input_result = input_result.map_err(|_| "stdin writer panicked")?;
            let stdout = out_result.map_err(|_| "stdout reader panicked")??;
            let stderr = err_result.map_err(|_| "stderr reader panicked")??;
            if reason.is_none() && exceeded.load(Ordering::Relaxed) {
                reason = Some("output_limit");
            }
            Ok(Execution {
                status: reason.unwrap_or(
                    if status.success() && input_result.is_ok() && !child.group_cleanup_required {
                        "succeeded"
                    } else {
                        "failed"
                    },
                ),
                code: status.code(),
                stdout,
                stderr,
                pid,
                reaped: true,
                group_cleanup_required: child.group_cleanup_required,
            })
        })();
        match result {
            Ok(result) => Ok(result),
            Err(error) => {
                let cleanup = child.terminate();
                let reaped = cleanup.is_ok();
                let diagnostic = match cleanup {
                    Ok(_) => format!("supervision failed after spawn; child reaped: {error}"),
                    Err(cleanup) => format!(
                        "supervision failed after spawn: {error}; cleanup unconfirmed: {cleanup}"
                    ),
                };
                // An already-started process is a failed execution, not refusal.
                // Normal callers can still persist a failure receipt and audit.
                Ok(Execution {
                    status: "failed",
                    code: None,
                    stdout: vec![],
                    stderr: diagnostic.into_bytes(),
                    pid,
                    reaped,
                    group_cleanup_required: child.group_cleanup_required,
                })
            }
        }
    })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod tape_ownership;
