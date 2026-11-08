//! Capability-authenticated requests on the task socket of the original service.
use super::*;
use crate::task_permissions::{self as grants, Binding, Credential, Permission, Scope};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    credential: Credential,
    request: Value,
}

pub(super) fn issue(root: &Path, req: &Value, parent: Option<Binding>) -> Result<Value> {
    let scope: Scope = serde_json::from_value(req["scope"].clone())?;
    grants::validate(&scope, parent.as_ref())?;
    let approved = req
        .get("approve")
        .map(|_| text(req, "approve"))
        .transpose()?;
    let (preview, execute) = approval(
        json!({"operation":"issue-task-grant", "root":root,
        "scope":scope,"parent":parent}),
        approved,
    )?;
    if !execute {
        return Ok(preview);
    }
    let (binding, credential) = grants::issue(root, scope, parent)?;
    Ok(json!({"grant":binding,"credential":credential}))
}

pub(super) fn revoke(root: &Path, req: &Value, actor: Option<&Binding>) -> Result<Value> {
    let target = grants::binding(root, text(req, "id")?)?;
    if let Some(actor) = actor
        && !grants::owns(actor, &target)?
    {
        return Err("cannot revoke another task grant".into());
    }
    let approved = req
        .get("approve")
        .map(|_| text(req, "approve"))
        .transpose()?;
    let (preview, execute) = approval(
        json!({"operation":"revoke-task-grant","grant":target}),
        approved,
    )?;
    if !execute {
        return Ok(preview);
    }
    grants::revoke(&target)?;
    reconcile(root)?;
    Ok(json!({"revoked":true,"grant":target,"termination_confirmed":false}))
}

pub(super) fn harness_permission(task: &harness::Task) -> Result<Permission> {
    let profile = json!({"binary":task.binary,"binary_sha256":task.binary_sha256,
        "schema":task.schema,"scope":task.scope,"model":task.model,"mcp":task.mcp});
    Ok(Permission::Harness {
        profile_sha256: store::digest(&serde_json::to_vec(&profile)?),
    })
}

pub(super) fn dispatch(root: &Path, envelope: &Value) -> Result<Value> {
    let envelope: Envelope = serde_json::from_value(envelope.clone())?;
    let actor = grants::authenticate(root, &envelope.credential)?;
    let req = &envelope.request;
    let op = text(req, "op")?;
    // Revocation retains status/cancellation access to the caller's own jobs.
    if !matches!(op, "task" | "grant-status" | "cancel" | "revoke-grant") {
        grants::active(&actor)?;
    }
    match op {
        "derive-grant" => return issue(root, req, Some(actor)),
        "revoke-grant" => return revoke(root, req, Some(&actor)),
        "grant-status" => {
            return Ok(json!({"grant":actor,"active":grants::active(&actor).is_ok()}));
        }
        "read-input" => {
            let path = Path::new(text(req, "path")?);
            let digest = text(req, "sha256")?;
            grants::require(
                &actor,
                &Permission::ReadInput {
                    path: path.into(),
                    sha256: digest.into(),
                },
            )?;
            let bytes = store::read(path, 512 * 1024)?;
            if store::digest(&bytes) != digest {
                return Err("authorized input changed".into());
            }
            grants::active(&actor)?;
            return Ok(json!({"sha256":digest,"bytes":bytes}));
        }
        "artifact-read" => {
            let instance = Path::new(text(req, "instance")?);
            let run = text(req, "run")?;
            let output = text(req, "output")?;
            grants::require(
                &actor,
                &Permission::Artifact {
                    instance: instance.into(),
                    run: run.into(),
                    output: output.into(),
                },
            )?;
            storage::instance(root, instance)?;
            let inventory = spool::artifact_output(instance, run, output)?;
            let binding = &inventory["output"]["binding"];
            let (bytes, _) = crate::artifact_versions::admission::read_input(
                Path::new(text(&inventory, "catalog")?),
                text(binding, "revision")?,
            )?;
            if bytes.len() > 512 * 1024 {
                return Err("task artifact exceeds 512 KiB transfer limit".into());
            }
            grants::active(&actor)?;
            return Ok(json!({"inventory":inventory,"bytes":bytes}));
        }
        "submit" => {
            let instance = PathBuf::from(text(req, "instance")?);
            let run = text(req, "run")?.into();
            let seconds = req["seconds"].as_u64().ok_or("seconds")?;
            let permission = if req.get("input").is_none() {
                let artifact = req.get("artifact").ok_or("input or artifact required")?;
                Permission::RunArtifact {
                    instance,
                    run,
                    seconds,
                    catalog: PathBuf::from(text(artifact, "catalog")?),
                    revision: text(artifact, "revision")?.into(),
                }
            } else {
                Permission::Run {
                    instance,
                    run,
                    seconds,
                    input: PathBuf::from(text(req, "input")?),
                }
            };
            grants::require(&actor, &permission)?;
        }
        "harness" | "delegate" => {
            let task: harness::Task = serde_json::from_value(req["task"].clone())?;
            grants::require(&actor, &harness_permission(&task)?)?;
            let scope = grants::load(&actor)?.scope;
            if op == "delegate" && req["parent"] != scope.task {
                return Err("delegation parent must match the authorized task".into());
            }
            if task.delegation.is_some() {
                return Err("use delegate for task-scoped submissions".into());
            }
        }
        "update-delegation" => {
            let job = owned_job(root, &actor, text(req, "id")?)?;
            let Job::Harness { task, .. } = job else {
                return Err("not a harness task".into());
            };
            grants::require(&actor, &harness_permission(&task)?)?;
        }
        "task" | "activity" | "harness-result" | "cancel" => {
            owned_job(root, &actor, text(req, "id")?)?;
        }
        "accept" | "record-adoption" => {
            if op == "record-adoption" {
                owned_job(root, &actor, text(req, "id")?)?;
            }
            grants::require(
                &actor,
                &Permission::Accept {
                    instance: PathBuf::from(text(req, "instance")?),
                    run: text(req, "run")?.into(),
                },
            )?;
        }
        "delegation-inbox" => {
            if req["parent"] != grants::load(&actor)?.scope.task {
                return Err("inbox belongs to another task".into());
            }
        }
        _ => return Err("operation is unavailable on the task permission endpoint".into()),
    }
    control::authorized(root, req, Some(&actor))
}

pub(super) fn owned_job(root: &Path, actor: &Binding, id: &str) -> Result<Job> {
    let (_, job) = storage::job(root, id)?;
    let binding = job
        .authorization()
        .ok_or("job has no task permission binding")?;
    if !grants::owns(actor, binding)? {
        return Err("job belongs to another task grant".into());
    }
    Ok(job)
}

/// Persist cancellation through the original owner, including after service restart.
pub(super) fn reconcile(root: &Path) -> Result<()> {
    for dir in storage::entries(root, "jobs", 256)? {
        let id = dir.file_name().and_then(|s| s.to_str()).ok_or("task ID")?;
        let (_, job) = storage::job(root, id)?;
        if job
            .authorization()
            .is_some_and(|b| grants::active(b).is_err())
            && scheduler::result(&job, &dir).is_err()
        {
            if !dir.join("cancel.json").try_exists()? {
                write(
                    &dir.join("cancel.json"),
                    &json!({"reason":"task permission unavailable","request":job.request()}),
                )?;
            }
            scheduler::cancel(&job)?;
        }
    }
    Ok(())
}
