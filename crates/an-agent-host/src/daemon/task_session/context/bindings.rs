//! Explicit instance grants. A primary agent cannot lend its larger grant to a spool.
use super::*;
use crate::task_permissions::{self as grants, Permission};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SpoolBinding {
    pub name: String,
    pub instance: PathBuf,
    pub release: String,
    pub grant: String,
    #[serde(default)]
    pub cocoon_jobs: Vec<JobBinding>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JobBinding {
    pub spec: PathBuf,
    pub sha256: String,
}

impl SpoolBinding {
    pub(super) fn verify(&self, authority: &Binding) -> Result<Binding> {
        grants::active(authority)?;
        if self.instance != self.instance.canonicalize()?
            || !store::valid_digest(&self.release)
            || self.cocoon_jobs.len() > 8
        {
            return Err("invalid fixed spool binding".into());
        }
        let state = crate::native_spool::show(&self.instance)?;
        if state["release"] != self.release || state["loaded"] != true {
            return Err("spool release changed or unloaded".into());
        }
        let child = grants::binding(&authority.root, &self.grant)?;
        if child == *authority {
            return Err("spool requires its own explicitly approved grant".into());
        }
        grants::active(&child)?;
        let scope = grants::load(&child)?.scope;
        if scope.delegate
            || scope.task != grants::load(authority)?.scope.task
            || scope.expires_ms > grants::load(authority)?.scope.expires_ms
        {
            return Err("nondelegating same-task spool grant required".into());
        }
        for permission in scope.permissions {
            match permission {
                Permission::Run { instance, .. }
                | Permission::RunArtifact { instance, .. }
                | Permission::RunCandidate { instance, .. }
                | Permission::RunJobOutput { instance, .. }
                | Permission::Accept { instance, .. }
                    if instance == self.instance => {}
                Permission::ReadInput { .. } | Permission::Artifact { .. } => (),
                Permission::CocoonJob { spec_sha256 }
                    if self.cocoon_jobs.iter().any(|j| j.sha256 == spec_sha256) => {}
                _ => return Err("spool grant contains unrelated or unsupported authority".into()),
            }
        }
        for job in &self.cocoon_jobs {
            #[cfg(unix)]
            {
                crate::cocoon::job::load(&job.spec, &job.sha256)?;
            }
            #[cfg(not(unix))]
            {
                let _ = job;
                return Err("Cocoon requires Unix".into());
            }
        }
        Ok(child)
    }
}

pub(super) fn projection(config: &Config, authority: &Binding) -> Result<Value> {
    let mut result = Vec::new();
    for binding in &config.spool_bindings {
        let grant = binding.verify(authority)?;
        let observed = crate::native_spool::show(&binding.instance)?;
        let checkpoint = if binding.instance.join("checkpoints/HEAD").exists() {
            crate::native_spool::checkpoint::inspect(&binding.instance, None)?["head"].clone()
        } else {
            Value::Null
        };
        let evidence = evidence(&grant)?;
        result.push(json!({"name":binding.name,"instance":observed["instance"],"release":binding.release,
            "state_revision":checkpoint,"grant_ref":{"id":grant.id,"sha256":grant.sha256},"effective_permissions":grants::load(&grant)?.scope.permissions,
            "requested_requirements":observed["requested_requirements"],"event_interface":observed["event_interface"],
            "operations":observed["runs"],"cocoon_specs":binding.cocoon_jobs.iter().map(|j|&j.sha256).collect::<Vec<_>>(),
            "evidence":evidence,"grants_authority":false}));
    }
    Ok(json!(result))
}

pub(super) fn select(config: &Config, authority: &Binding, req: &mut Value) -> Result<Binding> {
    if req.get("grant").is_some() {
        return Err("context tasks select spool bindings, not arbitrary grants".into());
    }
    let op = text(req, "op")?;
    if let Some(name) = req.get("spool_binding").and_then(Value::as_str) {
        let binding = config
            .spool_bindings
            .iter()
            .find(|b| b.name == name)
            .ok_or("unknown spool binding")?;
        let selected = binding.verify(authority)?;
        if let Some(instance) = req.get("instance")
            && op != "artifact-read"
            && instance != &json!(binding.instance)
        {
            return Err("spool target mismatch".into());
        }
        if op == "cocoon-job"
            && !binding
                .cocoon_jobs
                .iter()
                .any(|j| req["spec"] == json!(j.spec) && req["spec_sha256"] == j.sha256)
        {
            return Err("Cocoon job outside pinned spool declaration".into());
        }
        if matches!(
            op,
            "cocoon-call" | "cocoon-transfer" | "cocoon-reconcile" | "harness" | "delegate"
        ) {
            return Err("operation has no qualified spool declaration mapping".into());
        }
        req.as_object_mut()
            .ok_or("request")?
            .remove("spool_binding");
        return Ok(selected);
    }
    if req.get("instance").is_some()
        || matches!(
            op,
            "submit"
                | "accept"
                | "cocoon-job"
                | "cocoon-call"
                | "cocoon-transfer"
                | "cocoon-reconcile"
                | "harness"
                | "delegate"
        )
    {
        return Err("explicit spool binding required".into());
    }
    Ok(authority.clone())
}

pub(super) fn evidence(grant: &Binding) -> Result<Value> {
    let mut inputs = Vec::new();
    let mut artifacts = Vec::new();
    for permission in grants::load(grant)?.scope.permissions {
        match permission {
            Permission::ReadInput { path, sha256 } => {
                inputs.push(json!({"path":path,"sha256":sha256,"disclosure":"requires-receipt"}))
            }
            Permission::Artifact {
                instance,
                run,
                output,
            } if instance
                .join("runs")
                .join(&run)
                .join("accepted.json")
                .exists() =>
            {
                let report = crate::native_spool::artifact_output(&instance, &run, &output)?;
                artifacts.push(json!({"instance":report["instance"],"release":report["release"],"run":run,"output":output,
                        "revision":report["output"]["binding"]["revision"],"receipt_sha256":store::digest(&serde_json::to_vec(&report)?),"artifact_accepted":true}));
            }
            _ => (),
        }
    }
    Ok(json!({"inputs":inputs,"accepted_artifacts":artifacts}))
}
