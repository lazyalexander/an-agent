use super::*;
#[cfg(target_os = "linux")]
#[test]
fn inherited_writer_refuses_exec_with_preserved_errno_without_retry() -> Result<()> {
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
                    return Err("executable with an inherited writer unexpectedly launched".into());
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
    let published = root.join("published");
    let publication = super::super::publication::publish(
        &published,
        &std::fs::read(&executable)?,
        &AtomicBool::new(false),
    )?;
    assert!(publication.stopped.is_none());
    let next = execute(
        &published,
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
        include_bytes!("../../../tests/fixtures/execution_probe.rs"),
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
