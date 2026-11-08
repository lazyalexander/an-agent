//! User-level service entry; all controls use the private local transport.
use super::{Path, Result, Value};
use an_agent_host::daemon;

pub(super) fn dispatch(args: &[String]) -> Result<Value> {
    match args {
        [op, root, limit] if op == "init" => daemon::init(Path::new(root), limit.parse()?),
        [op, root] if op == "serve" => super::with_cancellation(|cancel| daemon::serve(Path::new(root), &cancel)),
        [op, root] if op == "show" => daemon::request(Path::new(root), &serde_json::json!({"op":"show"})),
        [op, root, output] if op == "launchd" => daemon::launchd(Path::new(root), Path::new(output)),
        [op, root, file] if op == "request" || op == "task-request" => {
            let bytes = std::fs::read(file)?;
            if bytes.len() > 131072 { return Err("daemon request exceeds 128 KiB".into()); }
            let request = serde_json::from_slice(&bytes)?;
            if op == "task-request" { daemon::task_request(Path::new(root), &request) }
            else { daemon::request(Path::new(root), &request) }
        }
        [op, root, id, hash] if op == "harness-worker" => super::with_cancellation(|cancel| {
            daemon::harness_worker(Path::new(root), id, hash, &cancel)
        }),
        _ => Err("usage: daemon init ROOT CONCURRENCY | serve ROOT | show ROOT | request ROOT JSON | task-request ROOT JSON | launchd ROOT PLIST".into()),
    }
}
