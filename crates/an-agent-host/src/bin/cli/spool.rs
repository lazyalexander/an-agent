//! Native spool commands use the same signal cancellation route as workspace jobs.
use super::{Path, Result, Value};
use an_agent_host::native_spool as spool;

pub(super) fn dispatch(command: &str, args: &[String]) -> Result<Value> {
    match (command, args) {
        ("spool-load", [package, root]) => spool::load(Path::new(package), Path::new(root)),
        ("spool-show", [root]) => spool::show(Path::new(root)),
        ("spool-kit-update", [root, name, source]) => {
            spool::update_kit(Path::new(root), name, Path::new(source), None)
        }
        ("spool-kit-update", [root, name, source, flag, hash]) if flag == "--approve" => {
            spool::update_kit(Path::new(root), name, Path::new(source), Some(hash))
        }
        ("spool-start", [root, name, input, seconds]) => start(root, name, input, seconds, None),
        ("spool-start", [root, name, input, seconds, flag, hash]) if flag == "--approve" => {
            start(root, name, input, seconds, Some(hash))
        }
        ("spool-unload", [root]) => spool::unload(Path::new(root), None),
        ("spool-unload", [root, flag, hash]) if flag == "--approve" => {
            spool::unload(Path::new(root), Some(hash))
        }
        ("spool-accept" | "spool-reclaim", [root, name]) => custody(command, root, name, None),
        ("spool-accept" | "spool-reclaim", [root, name, flag, hash]) if flag == "--approve" => {
            custody(command, root, name, Some(hash))
        }
        _ => Err("invalid native spool arguments; pause and suspend are not implemented".into()),
    }
}
fn start(
    root: &str,
    name: &str,
    input: &str,
    seconds: &str,
    approved: Option<&str>,
) -> Result<Value> {
    super::with_cancellation(|cancelled| {
        spool::start(
            Path::new(root),
            name,
            Path::new(input),
            seconds.parse()?,
            approved,
            &cancelled,
        )
    })
}
fn custody(command: &str, root: &str, name: &str, approved: Option<&str>) -> Result<Value> {
    if command == "spool-accept" {
        spool::accept(Path::new(root), name, approved)
    } else {
        spool::reclaim(Path::new(root), name, approved)
    }
}
