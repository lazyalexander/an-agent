//! Native dispatch cohort commands. Legacy CLI spellings use the same handlers.
use super::{Path, Result, Value};
use an_agent_host::native_spool::events::cohorts;
pub(super) fn dispatch(command: &str, args: &[String]) -> Result<Value> {
    let (args, approved) = match args {
        [rest @ .., flag, hash] if flag == "--approve" => (rest, Some(hash.as_str())),
        _ => (args, None),
    };
    match (command, args) {
        ("create", [root, name]) if approved.is_none() => cohorts::create(Path::new(root), name),
        ("show", [root, name]) if approved.is_none() => cohorts::inspect(Path::new(root), name),
        ("control", [root, name, mode]) => cohorts::change(Path::new(root), name, mode, approved),
        ("run", [root, name, member, request]) => super::with_cancellation(|cancel| {
            cohorts::run_supervised(
                Path::new(root),
                name,
                member,
                Path::new(request),
                approved,
                &cancel,
                &std::env::current_exe()?,
            )
        }),
        _ => Err("invalid cohort arguments".into()),
    }
}
