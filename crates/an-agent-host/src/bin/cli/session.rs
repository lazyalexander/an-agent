//! Foreground native session coordination. No harness or model loop.
use super::{Path, Result, Value};
use an_agent_host::native_spool::events::sessions;
pub(super) fn dispatch(command: &str, args: &[String]) -> Result<Value> {
    let (args, approved) = match args {
        [rest @ .., flag, hash] if flag == "--approve" => (rest, Some(hash.as_str())),
        _ => (args, None),
    };
    match (command, args) {
        ("session-create", [root, name]) if approved.is_none() => {
            sessions::create(Path::new(root), name)
        }
        ("session-show", [root, name]) if approved.is_none() => {
            sessions::inspect(Path::new(root), name)
        }
        ("session-control", [root, name, mode]) => {
            sessions::change(Path::new(root), name, mode, approved)
        }
        ("session-run", [root, name, thread, request]) => super::with_cancellation(|cancel| {
            sessions::run(
                Path::new(root),
                name,
                thread,
                Path::new(request),
                approved,
                &cancel,
            )
        }),
        _ => Err("invalid native session arguments".into()),
    }
}
