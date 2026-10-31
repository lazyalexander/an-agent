//! Trusted operator adapter; spool code never receives the host executable.
use super::{Path, Result, Value};
use an_agent_host::native_spool::peer;

pub(super) fn dispatch(args: &[String]) -> Result<Value> {
    let (args, approved) = match args {
        [rest @ .., flag, hash] if flag == "--approve" => (rest, Some(hash.as_str())),
        _ => (args, None),
    };
    match args {
        [op, root, name, file] if op == "request" => {
            peer::request(Path::new(root), name, Path::new(file), approved)
        }
        [op, root, name, phase] if op == "run" => super::with_cancellation(|cancel| {
            peer::execute(Path::new(root), name, phase, approved, &cancel)
        }),
        [op, root, name] if op == "reply" => peer::reply(Path::new(root), name, approved),
        [op, root, name] if op == "cancel" => peer::cancel(Path::new(root), name, approved),
        [op, root, name] if op == "show" && approved.is_none() => {
            peer::inspect(Path::new(root), name)
        }
        _ => Err("invalid spool-peer arguments".into()),
    }
}
