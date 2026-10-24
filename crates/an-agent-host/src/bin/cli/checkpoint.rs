use super::{Path, Result, Value};
pub(super) fn dispatch(command: &str, args: &[String]) -> Result<Value> {
    match (command, args) {
        ("checkpoint-commit", [store, id, expected, run, manifest]) => {
            Ok(an_agent_host::instance::checkpoint::commit(
                Path::new(store),
                id,
                expected,
                run,
                Path::new(manifest),
                None,
            )?)
        }
        ("checkpoint-commit", [store, id, expected, run, manifest, flag, digest])
            if flag == "--approve" =>
        {
            Ok(an_agent_host::instance::checkpoint::commit(
                Path::new(store),
                id,
                expected,
                run,
                Path::new(manifest),
                Some(digest),
            )?)
        }
        ("checkpoint-deliver", [store, publisher, revision, recipient]) => {
            super::with_cancellation(|cancelled| {
                an_agent_host::instance::checkpoint::deliver(
                    Path::new(store),
                    publisher,
                    revision,
                    recipient,
                    false,
                    &cancelled,
                )
            })
        }
        ("checkpoint-deliver", [store, publisher, revision, recipient, flag])
            if flag == "--execute" =>
        {
            super::with_cancellation(|cancelled| {
                an_agent_host::instance::checkpoint::deliver(
                    Path::new(store),
                    publisher,
                    revision,
                    recipient,
                    true,
                    &cancelled,
                )
            })
        }
        _ => Err(format!("invalid arguments for {command}").into()),
    }
}
