//! Async delegation metadata over the existing finite harness and FIFO queue.
use super::*;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Binding {
    pub parent: String,
    pub predecessor: Option<String>,
}

pub(super) fn validate(root: &Path, task: &harness::Task) -> Result<()> {
    let Some(binding) = &task.delegation else {
        return Ok(());
    };
    if binding.parent.is_empty() || binding.parent.len() > 120 {
        return Err("delegation requires a bounded parent reference".into());
    }
    if let Some(id) = &binding.predecessor {
        let (_, job) = storage::job(root, id)?;
        let Job::Harness { task: original, .. } = job else {
            return Err("delegation predecessor must be a harness task".into());
        };
        let previous = original.delegation.as_ref().ok_or("not a delegation")?;
        if previous.parent != binding.parent
            || original.scope != task.scope
            || original.schema != task.schema
            || original.binary != task.binary
            || original.binary_sha256 != task.binary_sha256
            || original.model != task.model
            || serde_json::to_value(&original.mcp)? != serde_json::to_value(&task.mcp)?
        {
            return Err("follow-up must retain the original disclosure scope and harness".into());
        }
    }
    Ok(())
}

pub(super) fn ready(root: &Path, job: &Job) -> Result<bool> {
    let Job::Harness { task, .. } = job else {
        return Ok(true);
    };
    let Some(id) = task
        .delegation
        .as_ref()
        .and_then(|b| b.predecessor.as_ref())
    else {
        return Ok(true);
    };
    let (dir, previous) = storage::job(root, id)?;
    // Unknown, failed or cancelled predecessors never authorize continuation.
    Ok(scheduler::result(&previous, &dir).is_ok_and(|r| r["status"] == "succeeded"))
}

pub(super) fn dispatch(root: &Path, req: &Value, approved: Option<&str>) -> Result<Value> {
    match text(req, "op")? {
        "delegate" => {
            let mut task: harness::Task = serde_json::from_value(req["task"].clone())?;
            task.delegation = Some(Box::new(Binding {
                parent: text(req, "parent")?.into(),
                predecessor: None,
            }));
            harness::submit(root, &json!({"task":task}), approved)
        }
        "update-delegation" => {
            if text(req, "strategy")? != "enqueue" {
                return Err(
                    "only explicit enqueue follow-ups are supported; no interrupt/replay".into(),
                );
            }
            let id = text(req, "id")?;
            let (_, job) = storage::job(root, id)?;
            let Job::Harness { mut task, .. } = job else {
                return Err("not a delegation".into());
            };
            let binding = task.delegation.as_mut().ok_or("not a delegation")?;
            binding.predecessor = Some(id.into());
            task.prompt.push_str(
                "\n\nFollow-up instruction (new finite attempt after predecessor succeeds):\n",
            );
            task.prompt.push_str(text(req, "instruction")?);
            harness::submit(root, &json!({"task":task}), approved)
        }
        _ => {
            let parent = text(req, "parent")?;
            let mut messages = Vec::new();
            for dir in storage::entries(root, "jobs", 256)? {
                let id = dir.file_name().and_then(|s| s.to_str()).ok_or("task ID")?;
                let (_, job) = storage::job(root, id)?;
                if let Job::Harness { task, .. } = &job
                    && task.delegation.as_ref().is_some_and(|b| b.parent == parent)
                {
                    let mut message = observation::task(root, id)?;
                    if scheduler::result(&job, &dir).is_ok_and(|r| r["proposal_available"] == true)
                    {
                        message["proposal"] = read(&dir.join("decision.json"))?;
                    }
                    messages.push(message);
                }
            }
            messages.sort_by_key(|row| row["order"].as_u64());
            Ok(json!({"parent":parent,"messages":messages,"artifact_accepted":false}))
        }
    }
}
