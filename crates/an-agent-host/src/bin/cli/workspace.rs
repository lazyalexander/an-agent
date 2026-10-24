use super::{Path, Result, Value};

pub(super) fn dispatch(command: &str, args: &[String]) -> Result<Value> {
    match (command, args) {
        ("workspace-run", [profile, root]) => run(profile, root, None, None),
        ("workspace-run", [profile, root, flag, digest]) if flag == "--approve" => {
            run(profile, root, None, Some(digest))
        }
        ("workspace-resume", [profile, root, prior]) => run(profile, root, Some(prior), None),
        ("workspace-resume", [profile, root, prior, flag, digest]) if flag == "--approve" => {
            run(profile, root, Some(prior), Some(digest))
        }
        ("workspace-accept" | "workspace-reclaim", [root, id]) => custody(command, root, id, None),
        ("workspace-accept" | "workspace-reclaim", [root, id, flag, digest])
            if flag == "--approve" =>
        {
            custody(command, root, id, Some(digest))
        }
        _ => Err("invalid workspace command arguments".into()),
    }
}

fn run(profile: &str, root: &str, prior: Option<&str>, approved: Option<&str>) -> Result<Value> {
    super::with_cancellation(|cancelled| {
        an_agent_host::workspace_job::run(
            Path::new(profile),
            Path::new(root),
            prior,
            approved,
            &cancelled,
        )
    })
}

fn custody(command: &str, root: &str, id: &str, approved: Option<&str>) -> Result<Value> {
    if command == "workspace-accept" {
        an_agent_host::workspace_job::accept(Path::new(root), id, approved)
    } else {
        an_agent_host::workspace_job::reclaim(Path::new(root), id, approved)
    }
}
