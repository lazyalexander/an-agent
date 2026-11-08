//! One-shot owner monitoring and client attachment. Losing the waiter does not stop the owner.
use super::*;
use std::{
    process::Child,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

/// A local service may wait elsewhere; the original finite owner still settles work.
pub(crate) fn launch_background(root: &Path, name: &str, hash: &str) -> Result<Child> {
    let (dir, original) = request(root, name)?;
    if original["approval_sha256"] != hash {
        return Err("queued supervisor identity changed".into());
    }
    crate::execution::supervisor_process::spawn(
        Path::new(text(&original["plan"], "executable")?),
        root,
        name,
        hash,
        &dir,
    )
}

pub fn launch(root: &Path, name: &str, prepared: &Value, cancelled: &AtomicBool) -> Result<Value> {
    if prepared["reserved"] != true {
        return Ok(prepared.clone());
    }
    let hash = text(prepared, "approval_sha256")?;
    let plan = &prepared["plan"];
    let root = location(root)?;
    let (dir, original) = request(&root, name)?;
    if original["plan"] != *plan || original["approval_sha256"] != hash {
        return Err("client launch differs from original supervisor reservation".into());
    }
    let mut child = crate::execution::supervisor_process::spawn(
        Path::new(text(plan, "executable")?),
        &root,
        name,
        hash,
        &dir,
    )?;
    let mut sent = false;
    wait(
        &mut child,
        plan["seconds"].as_u64().ok_or("deadline")? + 30,
        || {
            if cancelled.load(Ordering::Acquire) && !sent {
                let grant = cancel(&root, name, None)?;
                cancel(&root, name, Some(text(&grant, "approval_sha256")?))?;
                sent = true;
            }
            Ok(())
        },
    )?;
    completion(&root, name, hash)
}

#[allow(
    clippy::disallowed_methods,
    reason = "bounded client wait for the original independent owner"
)]
pub(in crate::native_spool) fn wait(
    child: &mut Child,
    seconds: u64,
    mut forward: impl FnMut() -> Result<()>,
) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        forward()?;
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("supervisor response unknown; inspect original work, never replay".into());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

struct StopObserver<'a>(&'a AtomicBool);
impl Drop for StopObserver<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

pub(in crate::native_spool) fn poll_control(
    dir: &Path,
    stop: Option<&AtomicBool>,
    cancel: &AtomicBool,
) {
    if dir.join("cancel.json").try_exists().unwrap_or(true) {
        cancel.store(true, Ordering::Release);
    }
    if let Some(stop) = stop
        && dir.join("stop.json").try_exists().unwrap_or(true)
    {
        stop.store(true, Ordering::Release);
    }
}

pub(in crate::native_spool) fn observe_controls<T>(
    dir: &Path,
    stop: Option<&AtomicBool>,
    cancel: &AtomicBool,
    body: impl FnOnce() -> Result<T>,
) -> Result<T> {
    monitor(|| poll_control(dir, stop, cancel), body)
}

#[allow(
    clippy::disallowed_methods,
    reason = "bounded owner control observation interval"
)]
pub(in crate::native_spool) fn monitor<T>(
    poll: impl Fn() + Sync,
    body: impl FnOnce() -> Result<T>,
) -> Result<T> {
    poll();
    let done = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let monitor = scope.spawn(|| {
            while !done.load(Ordering::Acquire) {
                poll();
                std::thread::sleep(Duration::from_millis(25));
            }
        });
        let guard = StopObserver(&done);
        let result = body();
        drop(guard);
        monitor.join().map_err(|_| "supervisor observer panicked")?;
        result
    })
}
