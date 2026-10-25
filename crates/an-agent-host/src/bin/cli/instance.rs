use super::{Path, Result, Value};
pub(super) fn dispatch(command: &str, args: &[String]) -> Result<Value> {
    match (command, args) {
        ("instance-init", [store, id, release, state]) => Ok(an_agent_host::instance::init(
            Path::new(store),
            id,
            release,
            Path::new(state),
            None,
        )?),
        ("instance-init", [store, id, release, state, flag, digest]) if flag == "--approve" => {
            Ok(an_agent_host::instance::init(
                Path::new(store),
                id,
                release,
                Path::new(state),
                Some(digest),
            )?)
        }
        ("instance-show", [store, id]) => Ok(an_agent_host::instance::inspect(
            Path::new(store),
            id,
            None,
        )?),
        ("instance-show", [store, id, revision]) => Ok(an_agent_host::instance::inspect(
            Path::new(store),
            id,
            Some(revision),
        )?),
        ("instance-run", [store, id, revision]) => super::with_cancellation(|cancelled| {
            an_agent_host::instance::run(Path::new(store), id, revision, false, &cancelled)
        }),
        ("instance-run", [store, id, revision, flag]) if flag == "--execute" => {
            super::with_cancellation(|cancelled| {
                an_agent_host::instance::run(Path::new(store), id, revision, true, &cancelled)
            })
        }
        ("instance-commit", [store, id, expected, run]) => Ok(an_agent_host::instance::commit(
            Path::new(store),
            id,
            expected,
            run,
            None,
        )?),
        ("instance-commit", [store, id, expected, run, flag, digest]) if flag == "--approve" => Ok(
            an_agent_host::instance::commit(Path::new(store), id, expected, run, Some(digest))?,
        ),
        _ => Err(format!("invalid arguments for {command}").into()),
    }
}
