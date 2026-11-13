//! Scheduled harness ownership. Proposals remain untrusted domain data.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct Task {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegation: Option<Box<super::delegation::Binding>>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub app_server: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter: Option<super::harness_adapter::Kind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_limit: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall_seconds: Option<u64>,
    pub label: String,
    pub binary: PathBuf,
    pub binary_sha256: String,
    pub prompt: String,
    pub schema: Value,
    pub scope: Value,
    pub model: Option<String>,
    pub mcp: Option<Mcp>,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct Mcp {
    pub url: String,
    pub token: String,
}

pub(super) fn submit(
    root: &Path,
    req: &Value,
    approved: Option<&str>,
    authorization: Option<&Binding>,
) -> Result<Value> {
    let task: Task = serde_json::from_value(req["task"].clone())?;
    super::delegation::validate(root, &task)?;
    let authorization = super::delegation::authorization(root, &task, authorization)?;
    let authorization = authorization.as_ref();
    if let Some(binding) = authorization {
        crate::task_permissions::require(binding, &permissions::harness_permission(&task)?)?;
    }
    if task.turn_limit.is_some_and(|limit| limit == 0 || limit > 8)
        || task
            .wall_seconds
            .is_some_and(|seconds| seconds == 0 || seconds > 3600)
        || (!super::harness_adapter::interactive(&task)? && task.turn_limit.unwrap_or(1) > 1)
        || task.label.is_empty()
        || task.label.len() > 120
        || task.prompt.len() > 65536
        || !task.schema.is_object()
        || task.binary != task.binary.canonicalize()?
        || task.binary_sha256
            != crate::execution::sandbox::service_process::executable_hash(&task.binary)?
    {
        return Err("invalid harness task or selected binary changed".into());
    }
    if let Some(mcp) = &task.mcp
        && (!loopback(&mcp.url)
            || mcp.token.is_empty()
            || mcp.token.len() > 8192
            || mcp.token.contains(['\r', '\n']))
    {
        return Err("finite material MCP must use an approved loopback endpoint".into());
    }
    if super::harness_adapter::interactive(&task)?
        && (authorization.is_none() || task.mcp.is_some())
    {
        return Err("app-server requires a task grant and no external MCP".into());
    }
    let binding = task_binding(&task, authorization)?;
    // Never copy the prompt, scoped content, or MCP secret into a public preview.
    let (preview, execute) = approval(
        json!({"operation":"queue-harness", "daemon":root,
        "task_sha256":binding,"label":task.label,"adapter":super::harness_adapter::kind(&task)?,"version":super::harness_adapter::selected(&task)?.capabilities().version,
        "disclosure":"exact submitted prompt/schema/scope; finite MCP if selected"}),
        approved,
    )?;
    if !execute {
        return Ok(preview);
    }
    let id = scheduler::enqueue(
        root,
        &Job::Harness {
            authorization: authorization.cloned(),
            task: Box::new(task),
            request: binding,
        },
    )?;
    Ok(json!({"id":id,"queued":true,"executed":false}))
}

fn task_binding(task: &Task, authorization: Option<&Binding>) -> Result<String> {
    let bytes = match authorization {
        Some(binding) => serde_json::to_vec(&json!({"task":task,"authorization":binding}))?,
        None => serde_json::to_vec(task)?,
    };
    Ok(store::digest(&bytes))
}

pub fn worker(root: &Path, id: &str, hash: &str, cancelled: &AtomicBool) -> Result<Value> {
    let root = storage::private(root)?;
    storage::config(&root)?;
    let (dir, job) = storage::job(&root, id)?;
    let Job::Harness {
        task,
        request,
        authorization,
    } = job
    else {
        return Err("not a harness task".into());
    };
    if request != hash
        || task_binding(&task, authorization.as_ref())? != hash
        || read(&dir.join("claimed.json"))?["request"] != hash
    {
        return Err("harness launch binding mismatch".into());
    }
    write(
        &dir.join("owner.json"),
        &json!({"request":hash,"pid":std::process::id(),
            "process_identity":crate::execution::sandbox::service_process::identity(std::process::id())}),
    )?;
    let work = dir.join("work");
    store::directory(&work)?;
    write(&dir.join("schema.json"), &task.schema)?;
    let result = spool::supervisor::monitor(
        || {
            if dir.join("cancel.json").exists()
                || authorization
                    .as_ref()
                    .is_some_and(|b| crate::task_permissions::active(b).is_err())
            {
                cancelled.store(true, Ordering::Release);
            }
        },
        || {
            super::harness_adapter::selected(&task)?.execute(
                root.as_path(),
                &dir,
                &task,
                authorization.as_ref(),
                cancelled,
            )
        },
    );
    let response = match &result {
        Ok(result) => json!({"request":hash,"result":result}),
        Err(error) => {
            json!({"request":hash,"error":error.to_string(),"termination_confirmed":false})
        }
    };
    write(&dir.join("completion.json"), &response)?;
    result
}

fn loopback(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://127.0.0.1:") else {
        return false;
    };
    let (port, path) = rest.split_once('/').unwrap_or((rest, ""));
    port.parse::<u16>().is_ok_and(|port| port > 0) && !path.contains(['\r', '\n', '#', '?'])
}

pub(super) fn completion(dir: &Path, hash: &str) -> Result<Value> {
    let value = read(&dir.join("completion.json"))?;
    if value["request"] != hash
        || read(&dir.join("owner.json"))?["request"] != hash
        || value["result"]["termination_confirmed"] != true
    {
        return Err("harness outcome unconfirmed; inspect original task, never replay".into());
    }
    Ok(value["result"].clone())
}
