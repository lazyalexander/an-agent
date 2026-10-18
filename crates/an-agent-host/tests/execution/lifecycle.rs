//! Real CLI signal delivery; no model or simulated success path.
use super::*;
use std::{
    process::{Child, Stdio},
    thread,
    time::{Duration, Instant},
};

struct HostProcess(Option<Child>);

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
        assert_eq!(
            execution::run(&fixture.store, &release, &fixture.input, true)?["status"],
            "succeeded"
        );
    }
    Ok(())
}
