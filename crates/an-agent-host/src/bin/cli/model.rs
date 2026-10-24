use super::{Path, Result, Value};
pub(super) fn dispatch(command: &str, args: &[String]) -> Result<Value> {
    match (command, args) {
        ("model", [profile, packet, output]) => super::with_cancellation(|cancelled| {
            an_agent_host::model_run::run_cancellable(
                Path::new(profile),
                Path::new(packet),
                Path::new(output),
                None,
                cancelled,
            )
        }),
        ("model", [profile, packet, output, flag, digest]) if flag == "--approve" => {
            super::with_cancellation(|cancelled| {
                an_agent_host::model_run::run_cancellable(
                    Path::new(profile),
                    Path::new(packet),
                    Path::new(output),
                    Some(digest),
                    cancelled,
                )
            })
        }
        _ => Err(format!("invalid arguments for {command}").into()),
    }
}
