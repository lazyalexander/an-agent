//! Foreground client for a one-shot native supervisor with durable control.
use super::{Path, Result, Value};
use an_agent_host::{execution::supervisor_process, native_spool::supervisor};

pub(super) fn dispatch(args: &[String]) -> Result<Value> {
    let (args, approved) = match args {
        [rest @ .., flag, hash] if flag == "--approve" => (rest, Some(hash.as_str())),
        _ => (args, None),
    };
    match args {
        [operation, root, name, input, seconds]
            if matches!(
                operation.as_str(),
                "start" | "start-state" | "start-data-state"
            ) =>
        {
            super::with_cancellation(|cancelled| {
                start(
                    Path::new(root),
                    name,
                    match operation.as_str() {
                        "start-state" => supervisor::Input::State(input),
                        "start-data-state" => supervisor::Input::DataState(Path::new(input)),
                        _ => supervisor::Input::File(Path::new(input)),
                    },
                    seconds.parse()?,
                    approved,
                    &cancelled,
                )
            })
        }
        [operation, root, name] if operation == "cancel" => {
            supervisor::cancel(Path::new(root), name, approved)
        }
        [operation, root, name] if operation == "show" && approved.is_none() => {
            supervisor::inspect(Path::new(root), name)
        }
        [operation, root, name, hash] if operation == "worker" && approved.is_none() => {
            super::with_cancellation(|cancelled| {
                supervisor::execute(Path::new(root), name, hash, cancelled)
            })
        }
        _ => Err("invalid spool-supervisor arguments".into()),
    }
}

#[allow(
    clippy::disallowed_methods,
    reason = "bounded client wait; native supervisor owns execution deadline"
)]
fn start(
    root: &Path,
    name: &str,
    input: supervisor::Input<'_>,
    seconds: u64,
    approved: Option<&str>,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<Value> {
    use std::{
        sync::atomic::Ordering,
        time::{Duration, Instant},
    };
    let executable = std::env::current_exe()?;
    let prepared = supervisor::prepare(root, name, input, seconds, &executable, approved)?;
    if prepared["reserved"] != true {
        return Ok(prepared);
    }
    let hash = prepared["approval_sha256"]
        .as_str()
        .ok_or("supervisor approval")?;
    let directory = supervisor::directory(root, name)?;
    let mut child = supervisor_process::spawn(&executable, root, name, hash, &directory)?;
    let deadline = Instant::now() + Duration::from_secs(seconds + 30);
    let mut cancellation_sent = false;
    loop {
        if cancelled.load(Ordering::Acquire) && !cancellation_sent {
            let plan = supervisor::cancel(root, name, None)?;
            supervisor::cancel(root, name, plan["approval_sha256"].as_str())?;
            cancellation_sent = true;
        }
        if child.try_wait()?.is_some() {
            // Worker exit alone grants nothing. Missing or inconsistent response
            // stays unknown and can be inspected without restarting the work.
            return supervisor::completion(root, name, hash);
        }
        if Instant::now() >= deadline {
            return Err("supervisor response unknown; inspect original run, never replay".into());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}
