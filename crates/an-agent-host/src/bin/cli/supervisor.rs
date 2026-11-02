//! Foreground client for a one-shot native supervisor with durable control.
use super::{Path, Result, Value};
use an_agent_host::native_spool::{
    events::coordinator::{self, Scope},
    supervisor,
};

pub(super) fn dispatch(args: &[String]) -> Result<Value> {
    let (args, approved) = match args {
        [rest @ .., flag, hash] if flag == "--approve" => (rest, Some(hash.as_str())),
        _ => (args, None),
    };
    match args {
        [op, root, name, hash] if op == "coordinator-worker" && approved.is_none() => {
            super::with_cancellation(|cancel| {
                coordinator::execute(Path::new(root), Scope::Dispatch(name), hash, &cancel)
            })
        }
        [op, root, name, thread, hash] if op == "coordinator-worker" && approved.is_none() => {
            super::with_cancellation(|cancel| {
                coordinator::execute(
                    Path::new(root),
                    Scope::in_cohort(name, thread),
                    hash,
                    &cancel,
                )
            })
        }
        [op, root, name, action] if op == "dispatch" => {
            coordinator::control(Path::new(root), Scope::Dispatch(name), action, approved)
        }
        [op, root, name, thread, action] if matches!(op.as_str(), "dispatch" | "thread") => {
            coordinator::control(
                Path::new(root),
                Scope::in_cohort(name, thread),
                action,
                approved,
            )
        }
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

fn start(
    root: &Path,
    name: &str,
    input: supervisor::Input<'_>,
    seconds: u64,
    approved: Option<&str>,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<Value> {
    let executable = std::env::current_exe()?;
    let prepared = supervisor::prepare(root, name, input, seconds, &executable, approved)?;
    supervisor::launch(root, name, &prepared, cancelled)
}
