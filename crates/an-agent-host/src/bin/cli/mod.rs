//! Small command-line adapter; parsers stay beside the host capability they invoke.
mod artifact;
mod checkpoint;
mod context;
mod host;
mod instance;
mod model;
mod retention;
mod spool;
mod workspace;

use an_agent_host::execution::Result;
use serde_json::Value;
use std::path::Path;

#[cfg(unix)]
#[derive(Default)]
struct SignalRegistrations(Vec<signal_hook::SigId>);

#[cfg(unix)]
impl Drop for SignalRegistrations {
    fn drop(&mut self) {
        for id in self.0.drain(..) {
            signal_hook::low_level::unregister(id);
        }
    }
}

fn with_cancellation(
    run: impl FnOnce(std::sync::Arc<std::sync::atomic::AtomicBool>) -> Result<Value>,
) -> Result<Value> {
    let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    #[cfg(unix)]
    let mut signals = SignalRegistrations::default();
    #[cfg(unix)]
    for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
        signals.0.push(signal_hook::flag::register(
            signal,
            std::sync::Arc::clone(&cancelled),
        )?);
    }
    run(cancelled)
}

pub(super) fn run(args: Vec<String>) -> Result<()> {
    let (command, args) = args.split_first().ok_or("missing command")?;
    let result = match command.as_str() {
        "instance-init" | "instance-show" | "instance-run" | "instance-commit" => {
            instance::dispatch(command, args)?
        }
        "checkpoint-commit" | "checkpoint-deliver" => checkpoint::dispatch(command, args)?,
        "artifact-import" | "artifact-checkout" | "artifact-edit" | "product-reclaim" => {
            artifact::dispatch(command, args)?
        }
        "work-reclaim" => retention::dispatch(command, args)?,
        "workspace-run" | "workspace-resume" | "workspace-accept" | "workspace-reclaim" => {
            workspace::dispatch(command, args)?
        }
        "spool-load"
        | "spool-show"
        | "spool-kit-update"
        | "spool-start"
        | "spool-accept"
        | "spool-reclaim"
        | "spool-unload"
        | "spool-kit-proposal-request"
        | "spool-kit-proposal-apply"
        | "spool-kit-proposal-revoke"
        | "spool-weaver-request"
        | "spool-weaver-apply"
        | "spool-weaver-revoke" => spool::dispatch(command, args)?,
        "context-check" | "context-export" => context::dispatch(command, args)?,
        "model" => model::dispatch(command, args)?,
        "check" | "register" | "run" | "run-version" | "run-openshell" | "build-kit"
        | "capability-read" => host::dispatch(command, args)?,
        _ => return Err("unknown command; consult the host command reference".into()),
    };
    println!("{}", serde_json::to_string(&result)?);
    if result
        .get("status")
        .and_then(|v| v.as_str())
        .is_some_and(|v| v != "succeeded")
    {
        return Err("execution did not succeed; inspect the retained receipt".into());
    }
    Ok(())
}
