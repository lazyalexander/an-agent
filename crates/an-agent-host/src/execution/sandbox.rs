//! A deliberately narrow native backend. No unsupported-platform fallback.
use std::io::{Read, Write};
use std::path::Path;
use std::process::Stdio;

mod macos;
pub use macos::profile;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use super::Result;

pub const INPUT_LIMIT: usize = 8192;
const OUTPUT_LIMIT: usize = 65536;

pub struct Execution {
    pub status: &'static str,
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

fn reader(mut pipe: impl Read, exceeded: Arc<AtomicBool>) -> std::io::Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let count = pipe.read(&mut chunk)?;
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
) -> Result<Execution> {
    if !cfg!(target_os = "macos") {
        return Err("no supported sandbox backend; refusing native execution".into());
    }
    if input.len() > INPUT_LIMIT {
        return Err("input exceeds 8192 bytes".into());
    }
    let executable = executable.canonicalize()?;
    let mut child = macos::command(&executable)?
        .current_dir(cwd)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take().ok_or("missing stdin")?;
    let stdout = child.stdout.take().ok_or("missing stdout")?;
    let stderr = child.stderr.take().ok_or("missing stderr")?;
    let exceeded = Arc::new(AtomicBool::new(false));
    let stdout_limit = Arc::clone(&exceeded);
    let stderr_limit = Arc::clone(&exceeded);
    // Concurrent drains and a writer keep full pipes from bypassing the deadline.
    let bytes = input.to_vec();
    let input_thread = thread::spawn(move || stdin.write_all(&bytes));
    let out_thread = thread::spawn(move || reader(stdout, stdout_limit));
    let err_thread = thread::spawn(move || reader(stderr, stderr_limit));
    // A native process deadline must follow real monotonic time, not replay time.
    #[allow(
        clippy::disallowed_methods,
        reason = "OS process wall-time enforcement"
    )]
    let start = Instant::now();
    let mut reason = None;
    let status = loop {
        if exceeded.load(Ordering::Relaxed) {
            reason = Some("output_limit");
        }
        if start.elapsed() >= timeout {
            reason = Some("timed_out");
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if reason.is_some() {
            child.kill()?;
            break child.wait()?;
        }
        thread::sleep(Duration::from_millis(10));
    };
    let input_result = input_thread.join().map_err(|_| "stdin writer panicked")?;
    let stdout = out_thread.join().map_err(|_| "stdout reader panicked")??;
    let stderr = err_thread.join().map_err(|_| "stderr reader panicked")??;
    if exceeded.load(Ordering::Relaxed) {
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "macos")]
    use std::process::Command;

    #[test]
    fn profile_is_deny_by_default_and_quotes_paths() -> Result<()> {
        let text = profile(Path::new("/tmp/a\"b"))?;
        assert!(text.contains("(deny default)"));
        assert!(!text.contains("allow network"));
        assert!(!text.contains("allow file-write"));
        assert!(!text.contains("allow process-fork"));
        assert!(text.contains("a\\\"b"));
        assert!(profile(Path::new("/tmp/a\nb")).is_err());
        Ok(())
    }

    #[test]
    #[cfg(target_os = "macos")]
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
        for input in [
            "write",
            "network",
            "spawn",
            "env",
            secret.to_str().ok_or("bad path")?,
        ] {
            let result = execute(&executable, &root, input.as_bytes(), Duration::from_secs(3))?;
            assert_eq!(
                result.status,
                "succeeded",
                "{input}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(result.stdout, b"denied as expected\n");
        }
        assert!(!root.join("forbidden.txt").exists());
        assert_eq!(
            execute(&executable, &root, b"hang", Duration::from_millis(150))?.status,
            "timed_out"
        );
        let flooded = execute(&executable, &root, b"flood", Duration::from_secs(3))?;
        assert_eq!(flooded.status, "output_limit");
        assert!(flooded.stdout.len() <= OUTPUT_LIMIT);
        std::fs::remove_dir_all(root)?;
        Ok(())
    }
}
