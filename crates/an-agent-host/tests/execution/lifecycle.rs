//! Real CLI signal delivery; no model or simulated success path.
use super::*;
use std::{
    process::{Child, Stdio},
    thread,
    time::{Duration, Instant},
};

struct HostProcess(Option<Child>);

#[cfg(target_os = "linux")]
#[test]
fn spawn_refusal_retains_error_context_and_audit_without_claiming_a_child() -> Result<()> {
    let fixture = Fixture::new()?;
    let path = fixture.deployment.join("program");
    let original = fs::read(&path)?;
    let yaml = fixture.deployment.join("spool.yaml");
    let declaration = fs::read_to_string(&yaml)?;
    let old_hash = format!("{:x}", Sha256::digest(&original));
    let bytes = b"#!/an-agent-test-missing-interpreter\n";
    fs::write(&path, bytes)?;
    fs::write(
        &yaml,
        declaration.replace(&old_hash, &format!("{:x}", Sha256::digest(bytes))),
    )?;
    let release = fixture.release()?;
    let result = execution::run(&fixture.store, &release, &fixture.input, true)?;
    assert_eq!(result["status"], "refused");
    let receipt_path = Path::new(result["receipt"].as_str().ok_or("missing receipt")?);
    let receipt: Value = serde_json::from_slice(&fs::read(receipt_path)?)?;
    assert_eq!(receipt["error"]["stage"], "spawn-or-pre-exec");
    assert_eq!(receipt["error"]["os_code"], libc::ENOENT);
    assert_eq!(receipt["error"]["retry_attempted"], false);
    assert!(receipt["process"]["pid"].is_null());
    assert_eq!(receipt["process"]["reaped"], false);
    let folder = receipt_path.parent().unwrap();
    assert_eq!(
        fs::read(folder.join("input.txt"))?,
        fs::read(&fixture.input)?
    );
    let audit = fs::read_to_string(folder.join("audit.json"))?;
    // This export is the thread tape, not the workspace event log.
    assert!(audit.contains("spool"));
    assert!(!audit.contains("\"applied\""));
    assert!(!fs::read(folder.join("stderr.txt"))?.is_empty());
    assert!(!fixture.store.join("host.lock").exists());
    assert_eq!(fs::read_dir(fixture.store.join("runs"))?.count(), 1);
    Ok(())
}

#[test]
fn prelaunch_cancellation_and_timeout_have_honest_process_evidence() -> Result<()> {
    let fixture = Fixture::new()?;
    let release = fixture.release()?;
    fs::write(&fixture.input, b"hang")?;
    let cancelled = execution::run_cancellable(
        &fixture.store,
        &release,
        &fixture.input,
        true,
        &std::sync::atomic::AtomicBool::new(true),
    )?;
    assert_eq!(cancelled["status"], "cancelled");
    let timed = execution::run(&fixture.store, &release, &fixture.input, true)?;
    assert_eq!(timed["status"], "timed_out");
    for result in [cancelled, timed] {
        let path = Path::new(result["receipt"].as_str().ok_or("missing receipt")?);
        let receipt: Value = serde_json::from_slice(&fs::read(path)?)?;
        if result["status"] == "cancelled" {
            assert!(receipt["process"]["pid"].is_null());
            assert_eq!(receipt["process"]["reaped"], false);
        } else {
            let pid = receipt["process"]["pid"]
                .as_u64()
                .ok_or("missing child PID")?;
            assert_eq!(receipt["process"]["reaped"], true);
            // SAFETY: query the direct child that this test just supervised.
            assert_eq!(
                unsafe { libc::waitpid(pid as i32, std::ptr::null_mut(), libc::WNOHANG) },
                -1
            );
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::ECHILD)
            );
        }
        assert!(
            !fs::read_to_string(path.parent().unwrap().join("audit.json"))?.contains("\"applied\"")
        );
    }
    assert!(!fixture.store.join("host.lock").exists());
    Ok(())
}
impl Drop for HostProcess {
    fn drop(&mut self) {
        if let Some(host) = self.0.as_mut() {
            // Test failure cleanup: enumerate only this still-owned host's children.
            if let Ok(output) = Command::new("pgrep")
                .args(["-P", &host.id().to_string()])
                .output()
            {
                for pid in String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .filter_map(|v| v.parse::<i32>().ok())
                {
                    if pid > 0 {
                        // SAFETY: signal the direct child just enumerated for this test host.
                        unsafe {
                            libc::kill(pid, libc::SIGKILL);
                        }
                    }
                }
            }
            let _ = host.kill();
            let _ = host.wait();
        }
    }
}

#[test]
#[allow(clippy::disallowed_methods, reason = "real process test deadline")]
fn cli_signals_reap_child_preserve_receipt_and_release_store_lock() -> Result<()> {
    // Isolate the writer/fork boundary from sibling Rust test threads. A fork
    // can retain their writable executable descriptors until its own exec,
    // producing ETXTBSY unrelated to cancellation (also covered in Codex's PTY
    // tests). This is process isolation, not a retry of a refused operation.
    const ISOLATED: &str = "AN_AGENT_TEST_ISOLATED_LIFECYCLE";
    if std::env::var_os(ISOLATED).is_none() {
        let output = Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "lifecycle::cli_signals_reap_child_preserve_receipt_and_release_store_lock",
                "--nocapture",
            ])
            .env(ISOLATED, "1")
            .output()?;
        assert!(
            output.status.success(),
            "isolated lifecycle test failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return Ok(());
    }
    let fixture = Fixture::new()?;
    let release = fixture.release()?;
    for signal in [libc::SIGINT, libc::SIGTERM] {
        fs::write(&fixture.input, b"hang")?;
        let child = Command::new(env!("CARGO_BIN_EXE_an-agent-host"))
            .arg("run")
            .arg(&fixture.store)
            .arg(&release)
            .arg(&fixture.input)
            .arg("--execute")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut host = HostProcess(Some(child));
        let host_pid = host.0.as_ref().unwrap().id();
        let start = Instant::now();
        let kit_pid = loop {
            let children = Command::new("pgrep")
                .args(["-P", &host_pid.to_string()])
                .output()?;
            if let Some(pid) = String::from_utf8_lossy(&children.stdout)
                .lines()
                .find_map(|s| s.parse::<u32>().ok())
            {
                break pid;
            }
            assert!(
                start.elapsed() < Duration::from_secs(4),
                "host did not launch kit"
            );
            thread::sleep(Duration::from_millis(10));
        };
        // SAFETY: signal the still-owned CLI process, not an arbitrary PID.
        assert_eq!(unsafe { libc::kill(host_pid as i32, signal) }, 0);
        let start = Instant::now();
        while host.0.as_mut().unwrap().try_wait()?.is_none() {
            assert!(
                start.elapsed() < Duration::from_secs(3),
                "cancel did not settle promptly"
            );
            thread::sleep(Duration::from_millis(10));
        }
        let output = host.0.take().unwrap().wait_with_output()?;
        assert!(!output.status.success());
        let text = String::from_utf8(output.stdout)?;
        let result: Value = serde_json::from_str(text.lines().last().ok_or("missing result")?)?;
        assert_eq!(result["status"], "cancelled");
        let receipt_path = Path::new(result["receipt"].as_str().ok_or("missing receipt")?);
        let receipt: Value = serde_json::from_slice(&fs::read(receipt_path)?)?;
        assert_eq!(receipt["process"]["pid"], kit_pid);
        assert_eq!(receipt["process"]["reaped"], true);
        // SAFETY: read-only existence check of the child recorded by this run.
        assert_eq!(unsafe { libc::kill(kit_pid as i32, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
        let audit = fs::read_to_string(receipt_path.parent().unwrap().join("audit.json"))?;
        assert!(!audit.contains("\"applied\""));
        assert!(!fixture.store.join("host.lock").exists());
        fs::write(&fixture.input, b"next run")?;
        // Both invocations use the real CLI. In-process library calls here
        // would otherwise test a different lifecycle than the cancelled CLI.
        let output = Command::new(env!("CARGO_BIN_EXE_an-agent-host"))
            .arg("run")
            .arg(&fixture.store)
            .arg(&release)
            .arg(&fixture.input)
            .arg("--execute")
            .output()?;
        let stdout = String::from_utf8(output.stdout)?;
        let next: Value =
            serde_json::from_str(stdout.lines().last().ok_or("missing next result")?)?;
        let next_receipt = Path::new(next["receipt"].as_str().ok_or("missing next receipt")?);
        assert_eq!(
            next["status"],
            "succeeded",
            "next run: {next}; receipt: {}; stderr: {}",
            fs::read_to_string(next_receipt)?,
            fs::read_to_string(next_receipt.parent().unwrap().join("stderr.txt"))?
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let next_receipt: Value = serde_json::from_slice(&fs::read(next_receipt)?)?;
        assert_eq!(next_receipt["process"]["reaped"], true);
        assert!(next_receipt.get("error").is_none());
        assert_ne!(next["receipt"], result["receipt"]);
        assert_eq!(
            fs::read(next["stdout"].as_str().ok_or("missing stdout")?)?,
            b"next run"
        );
        assert!(!fixture.store.join("host.lock").exists());
    }
    Ok(())
}
