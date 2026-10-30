//! Explicit native state commands; execution shares the existing signal owner.
use super::{Path, Result, Value};
use an_agent_host::native_spool::{checkpoint, start_state};

pub(super) fn dispatch(command: &str, args: &[String]) -> Result<Value> {
    match (command, args) {
        ("spool-state-init", [root, output, file]) => {
            checkpoint::init(Path::new(root), output, Path::new(file), None)
        }
        ("spool-state-init", [root, output, file, flag, hash]) if flag == "--approve" => {
            checkpoint::init(Path::new(root), output, Path::new(file), Some(hash))
        }
        ("spool-state-show", [root]) => checkpoint::inspect(Path::new(root), None),
        ("spool-state-show", [root, revision]) => {
            checkpoint::inspect(Path::new(root), Some(revision))
        }
        ("spool-state-commit", [root, expected, run]) => {
            checkpoint::commit(Path::new(root), expected, run, None)
        }
        ("spool-state-commit", [root, expected, run, flag, hash]) if flag == "--approve" => {
            checkpoint::commit(Path::new(root), expected, run, Some(hash))
        }
        ("spool-start-state", [root, run, revision, seconds]) => {
            start(root, run, revision, seconds, None)
        }
        ("spool-start-state", [root, run, revision, seconds, flag, hash])
            if flag == "--approve" =>
        {
            start(root, run, revision, seconds, Some(hash))
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
