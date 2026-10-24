use super::{Path, Result, Value};
pub(super) fn dispatch(command: &str, args: &[String]) -> Result<Value> {
    match (command, args) {
        ("artifact-import", [catalog, run]) => Ok(an_agent_host::artifact_versions::import(
            Path::new(catalog),
            Path::new(run),
        )?),
        ("artifact-checkout", [catalog, revision, output]) => {
            Ok(an_agent_host::artifact_versions::checkout(
                Path::new(catalog),
                revision,
                Path::new(output),
            )?)
        }
        ("artifact-edit", [catalog, parent, file, approved]) => {
            Ok(an_agent_host::artifact_versions::edit(
                Path::new(catalog),
                parent,
                Path::new(file),
                approved,
            )?)
        }
        ("product-reclaim", [store, publisher, revision]) => {
            Ok(an_agent_host::instance::checkpoint::reclaim(
                Path::new(store),
                publisher,
                revision,
                None,
            )?)
        }
        ("product-reclaim", [store, publisher, revision, flag, digest]) if flag == "--approve" => {
            Ok(an_agent_host::instance::checkpoint::reclaim(
                Path::new(store),
                publisher,
                revision,
                Some(digest),
            )?)
        }
        _ => Err(format!("invalid arguments for {command}").into()),
    }
}
