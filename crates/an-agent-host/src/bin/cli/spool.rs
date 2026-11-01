//! Native spool commands use the same signal cancellation route as workspace jobs.
use super::{Path, Result, Value};
use an_agent_host::native_spool as spool;

pub(super) fn dispatch(command: &str, args: &[String]) -> Result<Value> {
    match (command, args) {
        ("spool-release-diff", [previous, candidate]) => {
            spool::compare_releases(Path::new(previous), Path::new(candidate))
        }
        ("spool-storage-collect", [root, request]) => collect(root, request, None),
        ("spool-storage-collect", [root, request, flag, hash]) if flag == "--approve" => {
            collect(root, request, Some(hash))
        }
        ("spool-storage", [root]) => spool::storage(Path::new(root)),
        ("spool-reclaim-snapshots", [root, run]) => {
            spool::reclaim_snapshots(Path::new(root), run, None)
        }
        ("spool-reclaim-snapshots", [root, run, flag, hash]) if flag == "--approve" => {
            spool::reclaim_snapshots(Path::new(root), run, Some(hash))
        }
        ("spool-pause" | "spool-resume", [root]) => control(command, root, None),
        ("spool-pause" | "spool-resume", [root, flag, hash]) if flag == "--approve" => {
            control(command, root, Some(hash))
        }
        ("spool-suspend", [root, revision]) => spool::suspend(Path::new(root), revision, None),
        ("spool-suspend", [root, revision, flag, hash]) if flag == "--approve" => {
            spool::suspend(Path::new(root), revision, Some(hash))
        }
        ("spool-reconcile-run", [root, run]) => spool::reconcile_run(Path::new(root), run, None),
        ("spool-reconcile-run", [root, run, flag, hash]) if flag == "--approve" => {
            spool::reconcile_run(Path::new(root), run, Some(hash))
        }
        ("spool-reconcile-accept", [root, run]) => {
            spool::reconcile_accept(Path::new(root), run, None)
        }
        ("spool-reconcile-accept", [root, run, flag, hash]) if flag == "--approve" => {
            spool::reconcile_accept(Path::new(root), run, Some(hash))
        }
        ("spool-input-references", [root, run]) => {
            spool::input_references::inspect(Path::new(root), run)
        }
        ("spool-input-release", [root, run]) => {
            spool::input_references::release_cancelled(Path::new(root), run, None)
        }
        ("spool-input-release", [root, run, flag, hash]) if flag == "--approve" => {
            spool::input_references::release_cancelled(Path::new(root), run, Some(hash))
        }
        ("spool-input-release", [root, run, flag, workspace, subscription])
            if flag == "--delivery" =>
        {
            spool::input_references::release_succeeded(
                Path::new(root),
                run,
                Path::new(workspace),
                subscription,
                None,
            )
        }
        ("spool-input-release", [root, run, flag, workspace, subscription, approve, hash])
            if flag == "--delivery" && approve == "--approve" =>
        {
            spool::input_references::release_succeeded(
                Path::new(root),
                run,
                Path::new(workspace),
                subscription,
                Some(hash),
            )
        }
        (
            "spool-input-release",
            [
                root,
                run,
                flag,
                workspace,
                subscription,
                scope,
                file,
                rest @ ..,
            ],
        ) if flag == "--delivery" && scope == "--coverage" => {
            let approved = match rest {
                [] => None,
                [flag, hash] if flag == "--approve" => Some(hash.as_str()),
                _ => return Err("invalid stateful release approval".into()),
            };
            spool::input_references::release_checkpoint(
                Path::new(root),
                run,
                Path::new(workspace),
                subscription,
                Path::new(file),
                approved,
            )
        }
        ("spool-status", [root, run]) => spool::status(Path::new(root), run),
        ("spool-artifacts", [root, run]) => spool::artifacts(Path::new(root), run),
        ("spool-artifacts", [root, run, flag, output]) if flag == "--output" => {
            spool::artifact_output(Path::new(root), run, output)
        }
        ("spool-artifacts", [root, run, flag, output]) if flag == "--references" => {
            spool::artifact_references(Path::new(root), run, output)
        }
        ("spool-artifacts", [root, run, flag, output, consumers @ ..])
            if flag == "--review-retirement" =>
        {
            spool::review(Path::new(root), run, output, consumers)
        }
        ("spool-artifact-retire", [root, run, output]) => {
            spool::retire_artifact(Path::new(root), run, output, None)
        }
        ("spool-artifact-retire", [root, run, output, flag, hash]) if flag == "--approve" => {
            spool::retire_artifact(Path::new(root), run, output, Some(hash))
        }
        ("spool-kit-proposal-request" | "spool-weaver-request", [root, name]) => {
            spool::kit_proposal::request(Path::new(root), name)
        }
        ("spool-kit-proposal-revoke" | "spool-weaver-revoke", [root, id]) => {
            spool::kit_proposal::revoke(Path::new(root), id)
        }
        ("spool-kit-proposal-apply" | "spool-weaver-apply", [root, response]) => {
            spool::kit_proposal::apply(Path::new(root), Path::new(response), None)
        }
        ("spool-kit-proposal-apply" | "spool-weaver-apply", [root, response, flag, hash])
            if flag == "--approve" =>
        {
            spool::kit_proposal::apply(Path::new(root), Path::new(response), Some(hash))
        }
        ("spool-load", [package, root]) => spool::load(Path::new(package), Path::new(root)),
        ("spool-load", [package, root, flag, runtime]) if flag == "--runtime" => {
            spool::load_runtime(Path::new(package), Path::new(root), Path::new(runtime))
        }
        ("spool-show", [root]) => spool::show(Path::new(root)),
        ("spool-kit-reset", [root, name]) => spool::reset_kit(Path::new(root), name, None),
        ("spool-kit-reset", [root, name, flag, hash]) if flag == "--approve" => {
            spool::reset_kit(Path::new(root), name, Some(hash))
        }
        ("spool-kit-update", [root, name, source]) => {
            spool::update_kit(Path::new(root), name, Path::new(source), None)
        }
        ("spool-kit-update", [root, name, source, flag, hash]) if flag == "--approve" => {
            spool::update_kit(Path::new(root), name, Path::new(source), Some(hash))
        }
        ("spool-start", [root, name, input, seconds]) => start(root, name, input, seconds, None),
        ("spool-start", [root, name, input, seconds, flag, hash]) if flag == "--approve" => {
            start(root, name, input, seconds, Some(hash))
        }
        ("spool-unload", [root]) => spool::unload(Path::new(root), None),
        ("spool-unload", [root, flag, hash]) if flag == "--approve" => {
            spool::unload(Path::new(root), Some(hash))
        }
        ("spool-accept" | "spool-reclaim", [root, name]) => custody(command, root, name, None),
        ("spool-accept" | "spool-reclaim", [root, name, flag, hash]) if flag == "--approve" => {
            custody(command, root, name, Some(hash))
        }
        _ => Err(
            "invalid native spool arguments; live process pause/suspension is unsupported".into(),
        ),
    }
}
fn control(command: &str, root: &str, approved: Option<&str>) -> Result<Value> {
    if command == "spool-pause" {
        spool::pause(Path::new(root), approved)
    } else {
        spool::resume(Path::new(root), approved)
    }
}
fn start(
    root: &str,
    name: &str,
    input: &str,
    seconds: &str,
    approved: Option<&str>,
) -> Result<Value> {
    super::with_cancellation(|cancelled| {
        spool::start(
            Path::new(root),
            name,
            Path::new(input),
            seconds.parse()?,
            approved,
            &cancelled,
        )
    })
}
fn custody(command: &str, root: &str, name: &str, approved: Option<&str>) -> Result<Value> {
    if command == "spool-accept" {
        spool::accept(Path::new(root), name, approved)
    } else {
        spool::reclaim(Path::new(root), name, approved)
    }
}

fn collect(root: &str, request: &str, approved: Option<&str>) -> Result<Value> {
    super::with_cancellation(|cancelled| {
        spool::collect_private(Path::new(root), Path::new(request), approved, &cancelled)
    })
}
