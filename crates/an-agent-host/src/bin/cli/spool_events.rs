//! Explicit event operations reuse host approval and signal cancellation.
use super::{Path, Result, Value};
use an_agent_host::native_spool::events;

pub(super) fn dispatch(command: &str, args: &[String]) -> Result<Value> {
    let (args, approved) = match args {
        [rest @ .., flag, hash] if flag == "--approve" => (rest, Some(hash.as_str())),
        _ => (args, None),
    };
    match (command, args) {
        ("spool-events-dispatch", [root, request]) => dispatch_control(|stop, cancel| {
            events::dispatch(Path::new(root), Path::new(request), approved, stop, cancel)
        }),
        ("spool-events-dispatch-show", [root, name]) if approved.is_none() => {
            events::inspect_dispatch(Path::new(root), name)
        }
        ("spool-events-pending", [root]) if approved.is_none() => events::pending(Path::new(root)),
        ("spool-events-watch", [root, seconds, interval]) if approved.is_none() => {
            super::with_cancellation(|cancelled| {
                use std::io::Write;
                events::watch(
                    Path::new(root),
                    seconds.parse()?,
                    interval.parse()?,
                    &cancelled,
                    |value| {
                        let mut out = std::io::stdout().lock();
                        writeln!(out, "{}", serde_json::to_string(value)?)?;
                        out.flush()?;
                        Ok(())
                    },
                )
            })
        }
        ("spool-events-init", [root]) if approved.is_none() => events::init(Path::new(root)),
        ("spool-events-show", [root]) if approved.is_none() => events::inspect(Path::new(root)),
        ("spool-event-publish", [root, instance, run, output]) => {
            events::publish(Path::new(root), Path::new(instance), run, output, approved)
        }
        ("spool-event-subscribe", [root, name, config]) => {
            events::subscribe(Path::new(root), name, Path::new(config), approved)
        }
        ("spool-event-revoke", [root, name]) => events::revoke(Path::new(root), name, approved),
        ("spool-event-deliver", [root, event, name]) => super::with_cancellation(|cancelled| {
            events::deliver(Path::new(root), event, name, approved, &cancelled)
        }),
        ("spool-event-deliver", [root, event, name, flag, revision]) if flag == "--state" => {
            super::with_cancellation(|cancelled| {
                events::deliver_with_state(
                    Path::new(root),
                    event,
                    name,
                    revision,
                    approved,
                    &cancelled,
                )
            })
        }
        ("spool-event-complete", [root, event, name]) => {
            events::complete(Path::new(root), event, name, approved)
        }
        _ => Err("invalid native event arguments".into()),
    }
}

/// Dispatch stop drains the selected entry; cancel uses the existing supervisor.
fn dispatch_control(
    run: impl FnOnce(&std::sync::atomic::AtomicBool, &std::sync::atomic::AtomicBool) -> Result<Value>,
) -> Result<Value> {
    use std::sync::{Arc, atomic::AtomicBool};
    let stop = Arc::new(AtomicBool::new(false));
    let cancel = Arc::new(AtomicBool::new(false));
    #[cfg(unix)]
    let mut signals = super::SignalRegistrations::default();
    #[cfg(unix)]
    {
        signals.0.push(signal_hook::flag::register(
            signal_hook::consts::SIGTERM,
            Arc::clone(&stop),
        )?);
        signals.0.push(signal_hook::flag::register(
            signal_hook::consts::SIGINT,
            Arc::clone(&cancel),
        )?);
    }
    run(&stop, &cancel)
}
