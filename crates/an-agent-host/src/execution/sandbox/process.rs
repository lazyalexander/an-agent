//! Direct-child ownership. This is not a process-tree manager or a reaper service.
use std::{
    io,
    process::{Child, ExitStatus},
};

pub(super) struct OwnedChild {
    pub child: Child,
    reaped: Option<ExitStatus>,
}

impl OwnedChild {
    pub fn new(child: Child) -> Self {
        Self {
            child,
            reaped: None,
        }
    }

    pub fn poll(&mut self) -> io::Result<Option<ExitStatus>> {
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
