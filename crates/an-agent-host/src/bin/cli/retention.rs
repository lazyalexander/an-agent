use super::{Path, Result, Value};
pub(super) fn dispatch(command: &str, args: &[String]) -> Result<Value> {
    match (command, args) {
        ("work-reclaim", [store, run]) => Ok(an_agent_host::retention::reclaim(
            Path::new(store),
            run,
            None,
        )?),
        ("work-reclaim", [store, run, flag, digest]) if flag == "--approve" => Ok(
            an_agent_host::retention::reclaim(Path::new(store), run, Some(digest))?,
        ),
        _ => Err(format!("invalid arguments for {command}").into()),
    }
}
