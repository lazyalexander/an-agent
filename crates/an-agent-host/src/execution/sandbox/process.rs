//! Child ownership, with a macOS process-group scope for trusted compiler tools.
//! This is not arbitrary process-tree containment or a reaper service.
use std::{
    io,
    process::{Child, ExitStatus},
};

pub(super) struct OwnedChild {
    pub child: Child,
    reaped: Option<ExitStatus>,
    pub group_cleanup_required: bool,
    #[cfg(target_os = "macos")]
    group: bool,
}

impl OwnedChild {
    pub fn new(child: Child) -> Self {
        Self {
            child,
            reaped: None,
            group_cleanup_required: false,
            #[cfg(target_os = "macos")]
            group: false,
        }
    }

    #[cfg(target_os = "macos")]
    pub fn group(child: Child) -> Self {
        Self {
            child,
            reaped: None,
            group_cleanup_required: false,
            group: true,
        }
    }

    pub fn poll(&mut self) -> io::Result<Option<ExitStatus>> {
        #[cfg(target_os = "macos")]
        if self.group && self.reaped.is_none() {
            // Observe without reaping so the process-group ID cannot be reused
            // before all compiler children have been signalled.
            let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
            let result = unsafe {
                libc::waitid(
                    libc::P_PID,
                    self.child.id(),
                    info.as_mut_ptr(),
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if result != 0 {
                return Err(io::Error::last_os_error());
            }
            let info = unsafe { info.assume_init() };
            if info.si_pid != 0 {
                // A zero leader exit is not success if a silk is still owned by
                // the group. Killing leftover workers must not bless partial output.
                self.group_cleanup_required = group_has_peers(self.child.id())?;
                return self.terminate_group().map(Some);
            }
            return Ok(None);
        }
        if self.reaped.is_none() {
            self.reaped = self.child.try_wait()?;
        }
        Ok(self.reaped)
    }

    /// Killing is only a request. Success requires observing and reaping exit.
    pub fn terminate(&mut self) -> io::Result<ExitStatus> {
        if let Some(status) = self.reaped {
            return Ok(status);
        }
        #[cfg(target_os = "macos")]
        if self.group {
            return self.terminate_group();
        }
        match self.poll() {
            Ok(Some(status)) => return Ok(status),
            // Another reaper has consumed ownership. Never signal a numeric
            // PID that may now identify an unrelated process.
            #[cfg(unix)]
            Err(error) if error.raw_os_error() == Some(libc::ECHILD) => return Err(error),
            _ => (),
        }
        if let Err(error) = self.child.kill() {
            // A concurrent natural exit is not a cleanup failure.
            return match self.poll() {
                Ok(Some(status)) => Ok(status),
                _ => Err(error),
            };
        }
        let status = self.child.wait()?;
        self.reaped = Some(status);
        Ok(status)
    }

    #[cfg(target_os = "macos")]
    fn terminate_group(&mut self) -> io::Result<ExitStatus> {
        // Confirm child ownership before using a numeric group identity.
        let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
        if unsafe {
            libc::waitid(
                libc::P_PID,
                self.child.id(),
                info.as_mut_ptr(),
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        kill_owned_group(self.child.id() as i32)?;
        let status = self.child.wait()?;
        self.reaped = Some(status);
        Ok(status)
    }
}

#[cfg(target_os = "macos")]
fn group_has_peers(leader: u32) -> io::Result<bool> {
    // The unreaped leader pins the group identity. Two entries suffice to detect
    // another member; zombies are conservatively considered unfinished cleanup.
    let mut members = [0i32; 2];
    let count = unsafe {
        libc::proc_listpgrppids(
            leader as i32,
            members.as_mut_ptr().cast(),
            size_of_val(&members) as i32,
        )
    };
    if count < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(members.iter().any(|pid| *pid > 0 && *pid != leader as i32))
}

#[cfg(target_os = "macos")]
fn kill_owned_group(group: i32) -> io::Result<()> {
    // The caller retains an unreaped leader, pinning the numeric group identity.
    // macOS can deny a group signal even when individual members are signalable.
    if unsafe { libc::kill(-group, libc::SIGKILL) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::ESRCH) => return Ok(()),
        Some(libc::EPERM) => (),
        _ => return Err(error),
    }
    let mut members = vec![0i32; 16];
    loop {
        // SAFETY: writable PID buffer; the API returns the number of PIDs.
        let count = unsafe {
            libc::proc_listpgrppids(
                group,
                members.as_mut_ptr().cast(),
                (members.len() * size_of::<i32>()) as i32,
            )
        };
        if count < 0 {
            return Err(io::Error::last_os_error());
        }
        if (count as usize) < members.len() {
            members.truncate(count as usize);
            break;
        }
        if members.len() >= 65536 {
            return Err(io::Error::other("compiler group exceeds cleanup bound"));
        }
        members.resize(members.len() * 2, 0);
    }
    members.sort_unstable_by_key(|pid| *pid == group);
    let mut failure = None;
    for pid in members.into_iter().filter(|pid| *pid > 0) {
        // Recheck membership immediately before signaling; this is a trusted
        // compiler group, not containment for adversarial daemonization.
        let current = unsafe { libc::getpgid(pid) };
        if current == -1 || (current == group && unsafe { libc::kill(pid, libc::SIGKILL) } != 0) {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                failure.get_or_insert(error);
            }
        }
    }
    failure.map_or(Ok(()), Err)
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        // Error paths and unwinding retain ownership until kill/reap is attempted.
        // Fallible normal cleanup uses terminate() and reports any error.
        let _ = self.terminate();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        process::{Command, Stdio},
        thread,
    };

    fn assert_reaped(pid: u32) {
        // SAFETY: this only queries a direct child owned by this test.
        let result = unsafe { libc::waitpid(pid as i32, std::ptr::null_mut(), libc::WNOHANG) };
        assert_eq!(result, -1, "child still running or not reaped");
        assert_eq!(
            io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
    }

    #[test]
    fn error_and_panic_cleanup_precede_scoped_pipe_joins() {
        for panic in [false, true] {
            let mut pid = 0;
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                thread::scope(|scope| -> io::Result<()> {
                    let mut owned = OwnedChild::new(
                        Command::new("/bin/sleep")
                            .arg("30")
                            .stdin(Stdio::piped())
                            .stdout(Stdio::piped())
                            .spawn()?,
                    );
                    pid = owned.child.id();
                    let mut pipe = owned.child.stdout.take().unwrap();
                    let mut input = owned.child.stdin.take().unwrap();
                    scope.spawn(move || {
                        let _ = input.write_all(&vec![b'x'; 1024 * 1024]);
                    });
                    scope.spawn(move || {
                        let mut data = Vec::new();
                        pipe.read_to_end(&mut data).unwrap();
                    });
                    // Simulate setup/monitoring failure with a live pipe reader.
                    if panic {
                        panic!("injected supervisor unwind");
                    }
                    Err(io::Error::other("injected setup error"))
                })
            }));
            if panic {
                assert!(outcome.is_err());
            } else {
                assert!(outcome.unwrap().is_err());
            }
            assert_reaped(pid);
        }
    }

    #[test]
    fn confirmed_termination_is_idempotent() -> io::Result<()> {
        let mut owned = OwnedChild::new(Command::new("/bin/sleep").arg("30").spawn()?);
        let pid = owned.child.id();
        let first = owned.terminate()?;
        assert_eq!(first, owned.terminate()?);
        assert_reaped(pid);
        Ok(())
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[allow(clippy::disallowed_methods, reason = "real process cleanup deadline")]
    fn compiler_group_ends_descendant_pipes_before_reaping() -> io::Result<()> {
        use std::{
            os::unix::process::CommandExt,
            time::{Duration, Instant},
        };
        // The shell leaves a child holding stdout; normal leader exit must still
        // terminate that group before a pipe join can finish.
        let mut owner = OwnedChild::group(
            Command::new("/bin/sh")
                .args(["-c", "/bin/sleep 30 &"])
                .process_group(0)
                .stdout(Stdio::piped())
                .spawn()?,
        );
        let start = Instant::now();
        loop {
            if owner.poll()?.is_some() {
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(5));
            thread::sleep(Duration::from_millis(10));
        }
        let mut bytes = Vec::new();
        owner.child.stdout.take().unwrap().read_to_end(&mut bytes)?;
        assert!(start.elapsed() < Duration::from_secs(5));
        assert_reaped(owner.child.id());
        Ok(())
    }

    #[test]
    fn external_reaping_is_not_permission_to_signal_a_stale_pid() -> io::Result<()> {
        let mut owned = OwnedChild::new(Command::new("/usr/bin/true").spawn()?);
        let pid = owned.child.id() as i32;
        // SAFETY: deliberately simulate an embedding application's other reaper
        // consuming this test's own child. No unrelated process is touched.
        assert_eq!(unsafe { libc::waitpid(pid, std::ptr::null_mut(), 0) }, pid);
        assert_eq!(
            owned.terminate().unwrap_err().raw_os_error(),
            Some(libc::ECHILD)
        );
        Ok(())
    }
}
