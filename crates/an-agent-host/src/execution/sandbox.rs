//! A deliberately narrow native backend. No unsupported-platform fallback.
use std::io::{Read, Write};
use std::path::Path;
use std::process::Stdio;
mod process;
use process::OwnedChild;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(any(not(target_os = "linux"), test))]
mod macos;
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
    launch
        .current_dir(cwd)
        .env_clear()
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
        let mut child = OwnedChild::new(launch.spawn()?);
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
