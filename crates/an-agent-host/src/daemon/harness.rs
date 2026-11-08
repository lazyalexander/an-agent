//! Finite external Codex adapter. Proposals remain untrusted domain data.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) const VERSION: &str = "codex-cli 0.166.0";

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct Task {
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

pub(super) fn submit(root: &Path, req: &Value, approved: Option<&str>) -> Result<Value> {
    let task: Task = serde_json::from_value(req["task"].clone())?;
    if task.label.is_empty()
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
    let binding = store::digest(&serde_json::to_vec(&task)?);
    // Never copy the prompt, scoped content, or MCP secret into a public preview.
    let (preview, execute) = approval(
        json!({"operation":"queue-codex-harness", "daemon":root,
        "task_sha256":binding,"label":task.label,"version":VERSION,
        "disclosure":"exact submitted prompt/schema/scope; finite MCP if selected"}),
        approved,
    )?;
    if !execute {
        return Ok(preview);
    }
    let id = scheduler::enqueue(
        root,
        &Job::Harness {
            task,
            request: binding,
        },
    )?;
    Ok(json!({"id":id,"queued":true,"executed":false}))
}

pub fn worker(root: &Path, id: &str, hash: &str, cancelled: &AtomicBool) -> Result<Value> {
    let root = storage::private(root)?;
    storage::config(&root)?;
    let (dir, job) = storage::job(&root, id)?;
    let Job::Harness { task, request } = job else {
        return Err("not a harness task".into());
    };
    if request != hash
        || store::digest(&serde_json::to_vec(&task)?) != hash
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
    let stop = AtomicBool::new(false);
    let result = std::thread::scope(|scope| {
        let watch = scope.spawn(|| {
            while !stop.load(Ordering::Acquire) {
                if dir.join("cancel.json").exists() {
                    cancelled.store(true, Ordering::Release);
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
        });
        let result = execute(&dir, &task, cancelled);
        stop.store(true, Ordering::Release);
        let joined = watch.join();
        joined.map_err(|_| "harness control observer panicked")?;
        result
    });
    let response = match &result {
        Ok(result) => json!({"request":hash,"result":result}),
        Err(error) => {
            json!({"request":hash,"error":error.to_string(),"termination_confirmed":false})
        }
    };
    write(&dir.join("completion.json"), &response)?;
    result
}

fn execute(dir: &Path, task: &Task, cancelled: &AtomicBool) -> Result<Value> {
    if cancelled.load(Ordering::Acquire) {
        return Ok(
            json!({"status":"cancelled","termination_confirmed":true,"process_started":false,"proposal_available":false}),
        );
    }
    if crate::execution::sandbox::service_process::executable_hash(&task.binary)?
        != task.binary_sha256
    {
        return Err("harness executable changed after approval".into());
    }
    let version = crate::execution::sandbox::service_process::codex(
        task,
        dir,
        &["--version".into()],
        cancelled,
    )?;
    if version.status == "cancelled" {
        let confirmed = version.pid.is_none()
            || (version.reaped
                && crate::execution::sandbox::workspace::settlement(version.pid, version.reaped)["empty"]
                    == true);
        return Ok(
            json!({"status":"cancelled","termination_confirmed":confirmed,"proposal_available":false}),
        );
    }
    if version.status != "succeeded" || String::from_utf8(version.stdout)?.trim() != VERSION {
        return Err("harness requires qualified Codex CLI 0.166.0".into());
    }
    let mut args = vec![
        "exec".into(),
        "--ignore-user-config".into(),
        "--ignore-rules".into(),
        "--ephemeral".into(),
        "--skip-git-repo-check".into(),
        "--json".into(),
        "--output-schema".into(),
        dir.join("schema.json").display().to_string(),
        "--output-last-message".into(),
        dir.join("decision.json").display().to_string(),
    ];
    let mut settings = json!({"approval_policy":"never","default_permissions":"finite",
        "permissions":{"finite":{"filesystem":{":root":"none",":project_roots":"read"},"network":{"enabled":false}}},
        "project_doc_max_bytes":0,"web_search":"disabled",
        "features":{"shell_tool":false,"view_image":false,"multi_agent":false,"apps":false,
            "plugins":false,"remote_plugin":false,"hooks":false,"goals":false,"skill_search":false,
            "skip_host_skill_discovery":true,"multi_agent_v2":{"enabled":false,"disable_native_tools":true}}});
    if let Some(mcp) = &task.mcp {
        settings["mcp_servers"] = json!({"material":{"url":mcp.url,"http_headers":{"Authorization":format!("Bearer {}",mcp.token)},
            "enabled_tools":["read_approved_material"],"required":true}});
    }
    for (key, value) in settings.as_object().ok_or("settings")? {
        args.extend(["-c".into(), format!("{key}={}", toml(value))]);
    }
    if let Some(model) = &task.model {
        args.extend(["--model".into(), model.clone()]);
    }
    // A prompt beginning with '-' is data, never another native-client option.
    args.extend(["--".into(), task.prompt.clone()]);
    let result = crate::execution::sandbox::service_process::codex(task, dir, &args, cancelled)?;
    let redact = |bytes: Vec<u8>| {
        let text = String::from_utf8_lossy(&bytes).into_owned();
        task.mcp
            .as_ref()
            .map_or(text.clone(), |mcp| text.replace(&mcp.token, "[redacted]"))
    };
    store::write_new(
        &dir.join("native-events.jsonl"),
        redact(result.stdout).as_bytes(),
    )?;
    store::write_new(
        &dir.join("diagnostic.txt"),
        redact(result.stderr).as_bytes(),
    )?;
    let settlement = crate::execution::sandbox::workspace::settlement(result.pid, result.reaped);
    let confirmed = (result.pid.is_none() && result.status == "cancelled")
        || (result.reaped && settlement["empty"] == true);
    Ok(
        json!({"status":result.status,"code":result.code,"termination_confirmed":confirmed,
        "group_observation":settlement,"group_cleanup_required":result.group_cleanup_required,
        "proposal_available":result.status=="succeeded" && confirmed && dir.join("decision.json").is_file(),
        "artifact_accepted":false,"remote_termination_confirmed":false,"automatic_retries":0}),
    )
}

fn toml(value: &Value) -> String {
    match value {
        Value::Object(map) => format!(
            "{{{}}}",
            map.iter()
                .map(|(k, v)| format!("{}={}", json!(k), toml(v)))
                .collect::<Vec<_>>()
                .join(",")
        ),
        Value::Array(items) => {
            format!("[{}]", items.iter().map(toml).collect::<Vec<_>>().join(","))
        }
        _ => value.to_string(),
    }
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
