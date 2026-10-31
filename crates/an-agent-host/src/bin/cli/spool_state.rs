//! Explicit native state commands; execution shares the existing signal owner.
use super::{Path, Result, Value};
use an_agent_host::native_spool::{checkpoint, start_data_state, start_state};

pub(super) fn dispatch(command: &str, args: &[String]) -> Result<Value> {
    let (args, approved) = match args {
        [rest @ .., flag, hash] if flag == "--approve" => (rest, Some(hash.as_str())),
        _ => (args, None),
    };
    if let ("spool-start-data-state", [root, run, selection, seconds]) = (command, args) {
        return super::with_cancellation(|cancelled| {
            start_data_state(
                Path::new(root),
                run,
                Path::new(selection),
                seconds.parse()?,
                approved,
                &cancelled,
            )
        });
    }
    match (command, args) {
        ("spool-state-dependencies", [root, revision, request]) => {
            checkpoint::declare(Path::new(root), revision, Path::new(request), approved)
        }
        ("spool-state-init", [root, output, file]) => {
            checkpoint::init(Path::new(root), output, Path::new(file), approved)
        }
        ("spool-state-show", [root]) if approved.is_none() => {
            checkpoint::inspect(Path::new(root), None)
        }
        ("spool-state-show", [root, revision]) if approved.is_none() => {
            checkpoint::inspect(Path::new(root), Some(revision))
        }
        ("spool-state-commit", [root, expected, run]) => {
            checkpoint::commit(Path::new(root), expected, run, approved)
        }
        ("spool-start-state", [root, run, revision, seconds]) => {
            start(root, run, revision, seconds, approved)
        }
        _ => Err("invalid native state arguments".into()),
    }
}

fn start(
    root: &str,
    run: &str,
    revision: &str,
    seconds: &str,
    hash: Option<&str>,
) -> Result<Value> {
    super::with_cancellation(|cancelled| {
        start_state(
            Path::new(root),
            run,
            revision,
            seconds.parse()?,
            hash,
            &cancelled,
        )
    })
}
