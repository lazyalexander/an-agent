//! A deliberately narrow native backend. No unsupported-platform fallback.
use std::io::{Read, Write};
use std::path::Path;
use std::process::Stdio;
mod process;
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
}

/// The standard process API reports spawn and pre-exec failures together.
/// Do not claim a child was launched or attribute an errno to a specific policy.
#[derive(Debug, thiserror::Error)]
#[error("spawn or pre-exec refused: {0}")]
struct LaunchError(#[source] std::io::Error);

pub(super) fn refusal_evidence(error: &(dyn std::error::Error + 'static)) -> serde_json::Value {
    let mut current = Some(error);
    let mut os_error = None;
    let mut stage = "sandbox-setup";
    while let Some(cause) = current {
        if cause.is::<LaunchError>() {
            stage = "spawn-or-pre-exec";
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
                status: reason.unwrap_or(if status.success() && input_result.is_ok() {
                    "succeeded"
                } else {
                    "failed"
                }),
                code: status.code(),
                stdout,
                stderr,
                pid,
                reaped: true,
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
                })
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    #[test]
    fn inherited_writer_refuses_exec_with_preserved_errno_without_retry() -> Result<()> {
        const ISOLATED: &str = "AN_AGENT_TEST_ISOLATED_WRITER";
        if std::env::var_os(ISOLATED).is_none() {
            let output = Command::new(std::env::current_exe()?)
                .args(["--exact", "execution::sandbox::tests::inherited_writer_refuses_exec_with_preserved_errno_without_retry", "--nocapture"])
                .env(ISOLATED, "1").output()?;
            assert!(output.status.success(), "writer probe failed: {output:?}");
            return Ok(());
        }
        use std::os::{
            fd::AsRawFd,
            unix::{fs::PermissionsExt, net::UnixStream, process::CommandExt},
        };
        let root = std::env::temp_dir().join(format!(
            "host-text-busy-{}",
            an_agent_core::det_seam::Entropy::os().uuid_v4()
        ));
        std::fs::create_dir(&root)?;
        let executable = root.join("program");
        std::fs::copy("/usr/bin/true", &executable)?;
        let writer = std::fs::OpenOptions::new().write(true).open(&executable)?;
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o500))?;
        let (mut parent, child) = UnixStream::pair()?;
        parent.set_read_timeout(Some(Duration::from_secs(3)))?;
        let parent_fd = parent.as_raw_fd();
        let child_fd = child.as_raw_fd();
        let mut sibling = Command::new("/usr/bin/true");
        // SAFETY: raw close/read/write only after fork. The pipe synchronizes an
        // inherited writable FD deterministically, without sleeps or retries.
        unsafe {
            sibling.pre_exec(move || {
                libc::close(parent_fd);
                let mut byte = 1_u8;
                if libc::write(child_fd, (&byte as *const u8).cast(), 1) != 1 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::read(child_fd, (&mut byte as *mut u8).cast(), 1) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        thread::scope(|scope| -> Result<()> {
            let sibling = scope.spawn(move || {
                let result = sibling.status();
                drop(child);
                result
            });
            let result = (|| -> Result<()> {
                parent.read_exact(&mut [0_u8])?;
                drop(writer); // Only the sibling's pre-exec copy remains open.
                let error = match execute(
                    &executable,
                    &root,
                    b"",
                    Duration::from_secs(1),
                    &AtomicBool::new(false),
                ) {
                    Err(error) => error,
                    Ok(_) => {
                        return Err(
                            "executable with an inherited writer unexpectedly launched".into()
                        );
                    }
                };
                let evidence = refusal_evidence(error.as_ref());
                if evidence["stage"] != "spawn-or-pre-exec"
                    || evidence["os_code"] != libc::ETXTBSY
                    || evidence["retry_attempted"] != false
                {
                    return Err(format!("unexpected refusal evidence: {evidence}").into());
                }
                Ok(())
            })();
            // EOF releases the sibling even when a preceding assertion fails.
            drop(parent);
            assert!(sibling.join().map_err(|_| "sibling panicked")??.success());
            result
        })?;
        let next = execute(
            &executable,
            &root,
            b"",
            Duration::from_secs(1),
            &AtomicBool::new(false),
        )?;
        assert_eq!(next.status, "succeeded");
        assert_reaped(&next);
        std::fs::remove_dir_all(root)?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn compiler_group_timeout_and_cancel_use_the_existing_supervisor() -> Result<()> {
        use std::os::unix::process::CommandExt;
        for cancel in [false, true] {
            let cancelled = AtomicBool::new(false);
            thread::scope(|scope| -> Result<()> {
                scope.spawn(|| {
                    if cancel {
                        thread::sleep(Duration::from_millis(150));
                        cancelled.store(true, Ordering::Relaxed);
                    }
                });
                let mut command = std::process::Command::new("/bin/sh");
                command
                    .args(["-c", "/bin/sleep 30 & wait"])
                    .env_clear()
                    .process_group(0);
                let result = execute_command(
                    command,
                    Path::new("/"),
                    b"",
                    Duration::from_millis(300),
                    &cancelled,
                    true,
                )?;
                assert_eq!(
                    result.status,
                    if cancel { "cancelled" } else { "timed_out" }
                );
                assert_reaped(&result);
                Ok(())
            })?;
        }
        Ok(())
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    use std::process::Command;

    #[test]
    fn interrupted_read_is_not_a_supervision_failure() -> Result<()> {
        struct InterruptedOnce(bool);
        impl Read for InterruptedOnce {
            fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
                if !self.0 {
                    self.0 = true;
                    return Err(std::io::ErrorKind::Interrupted.into());
                }
                let _ = output;
                Ok(0)
            }
        }
        assert!(reader(InterruptedOnce(false), Arc::new(AtomicBool::new(false)))?.is_empty());
        Ok(())
    }

    #[cfg(unix)]
    fn assert_reaped(result: &Execution) {
        assert!(result.reaped);
        let pid = result.pid.expect("execution must have started");
        // SAFETY: query only the direct child whose ownership just settled.
        assert_eq!(
            unsafe { libc::waitpid(pid as i32, std::ptr::null_mut(), libc::WNOHANG) },
            -1
        );
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
    }

    #[test]
    fn profile_is_deny_by_default_and_quotes_paths() -> Result<()> {
        let text = macos::profile(Path::new("/tmp/a\"b"))?;
        assert!(text.contains("(deny default)"));
        assert!(!text.contains("allow network"));
        assert!(!text.contains("allow file-write"));
        assert!(!text.contains("allow process-fork"));
        assert!(text.contains("a\\\"b"));
        assert!(macos::profile(Path::new("/tmp/a\nb")).is_err());
        Ok(())
    }

    #[test]
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn actual_backend_denies_effects_and_bounds_output_and_time() -> Result<()> {
        let root = std::env::temp_dir().join(format!(
            "host-sandbox-{}",
            an_agent_core::det_seam::Entropy::os().uuid_v4()
        ));
        std::fs::create_dir(&root)?;
        let source = root.join("probe.rs");
        let executable = root.join("probe");
        std::fs::write(
            &source,
            include_bytes!("../../tests/fixtures/execution_probe.rs"),
        )?;
        let status = Command::new("rustc")
            .arg("--edition=2024")
            .arg(&source)
            .arg("-o")
            .arg(&executable)
            .status()?;
        assert!(status.success());
        let secret = root.join("host-secret.txt");
        std::fs::write(&secret, b"test-only host data")?;
        // Positive controls: the surrounding environment permits these effects.
        assert_eq!(std::fs::read(&secret)?, b"test-only host data");
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        drop(listener);
        for input in [
            "write",
            "network",
            "spawn",
            "env",
            secret.to_str().ok_or("bad path")?,
        ] {
            let result = execute(
                &executable,
                &root,
                input.as_bytes(),
                Duration::from_secs(3),
                &AtomicBool::new(false),
            )?;
            assert_eq!(
                result.status,
                "succeeded",
                "{input}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(result.stdout, b"denied as expected\n");
        }
        assert!(!root.join("forbidden.txt").exists());
        let timed = execute(
            &executable,
            &root,
            b"hang",
            Duration::from_millis(150),
            &AtomicBool::new(false),
        )?;
        assert_eq!(timed.status, "timed_out");
        assert_reaped(&timed);
        let flooded = execute(
            &executable,
            &root,
            b"flood",
            Duration::from_secs(3),
            &AtomicBool::new(false),
        )?;
        assert_eq!(flooded.status, "output_limit");
        assert!(flooded.stdout.len() <= OUTPUT_LIMIT);
        assert_reaped(&flooded);
        let cancelled = AtomicBool::new(false);
        thread::scope(|scope| -> Result<()> {
            scope.spawn(|| {
                thread::sleep(Duration::from_millis(150));
                cancelled.store(true, Ordering::Relaxed);
            });
            let stopped = execute(
                &executable,
                &root,
                b"hang",
                Duration::from_secs(3),
                &cancelled,
            )?;
            assert_eq!(stopped.status, "cancelled");
            assert_reaped(&stopped);
            Ok(())
        })?;
        let before_spawn = execute(
            &executable,
            &root,
            b"hang",
            Duration::ZERO,
            &AtomicBool::new(true),
        )?;
        assert_eq!(before_spawn.status, "cancelled");
        assert!(before_spawn.pid.is_none());
        // Child confinement must not restrict the parent.
        assert_eq!(std::fs::read(&secret)?, b"test-only host data");
        std::fs::remove_dir_all(root)?;
        Ok(())
    }
}
