//! Explicit event operations reuse host approval and signal cancellation.
use super::{Path, Result, Value};
use an_agent_host::native_spool::events;

pub(super) fn dispatch(command: &str, args: &[String]) -> Result<Value> {
    let (args, approved) = match args {
        [rest @ .., flag, hash] if flag == "--approve" => (rest, Some(hash.as_str())),
        _ => (args, None),
    };
    match (command, args) {
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
