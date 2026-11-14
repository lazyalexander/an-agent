//! Qualified Codex launch profiles behind the host adapter contract.
use super::*;
use std::sync::atomic::Ordering;

const VERSION: &str = "codex-cli 0.166.0";

pub(super) struct Codex {
    pub(super) interactive: bool,
}
impl super::harness_adapter::HarnessAdapter for Codex {
    fn capabilities(&self) -> super::harness_adapter::Capabilities {
        super::harness_adapter::Capabilities {
            version: VERSION,
            interactive: self.interactive,
            steer: self.interactive,
            continue_turn: self.interactive,
            finish_session: self.interactive,
            reconnect_observation: true,
            resume_execution: false,
            cancel: "host-process-group",
            usage: "provider-cumulative-snapshots",
        }
    }
    fn execute(
        &self,
        root: &Path,
        dir: &Path,
        task: &harness::Task,
        authority: Option<&Binding>,
        cancelled: &AtomicBool,
    ) -> Result<Value> {
        execute(root, dir, task, authority, cancelled)
    }
    fn event(&self, native: &Value) -> Value {
        event(native)
    }
}

pub(super) fn execute(
    root: &Path,
    dir: &Path,
    task: &harness::Task,
    authorization: Option<&Binding>,
    cancelled: &AtomicBool,
) -> Result<Value> {
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
    if super::harness_adapter::interactive(task)? {
        return super::app_server::execute(
            root,
            dir,
            task,
            authorization.ok_or("missing app-server authority")?,
            cancelled,
        );
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

pub(super) fn toml(value: &Value) -> String {
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

/// Stable observation categories omit raw prompts, arguments and credentials.
pub(super) fn event(native: &Value) -> Value {
    let method = native["method"]
        .as_str()
        .or(native["type"].as_str())
        .unwrap_or("");
    let category = match method {
        "thread/started" | "thread.started" => "session-started",
        "turn/started" | "turn.started" => "turn-started",
        "turn/completed" | "turn.completed" => "turn-completed",
        "error" | "turn.failed" => "error",
        "item/tool/call" => "tool-request",
        "item/started" | "item.started" => "item-started",
        "item/completed" | "item.completed" => "item-completed",
        "thread/tokenUsage/updated" => "usage",
        _ => "other",
    };
    let mut event =
        json!({"event":category,"evidence":"external-harness-report","grants_authority":false});
    if let Some(usage) = usage(native) {
        event["usage"] = usage;
    }
    event["observed_ms"] = native["host_observed_ms"].clone();
    event
}

pub(super) fn usage(native: &Value) -> Option<Value> {
    let (usage, session, turn, names) = match native["method"].as_str() {
        Some("thread/tokenUsage/updated") => (
            &native["params"]["tokenUsage"]["total"],
            native["params"]["threadId"].as_str()?,
            native["params"]["turnId"].as_str(),
            [
                "inputTokens",
                "cachedInputTokens",
                "outputTokens",
                "reasoningOutputTokens",
                "totalTokens",
            ],
        ),
        _ if native["type"] == "turn.completed" && native["usage"].is_object() => (
            &native["usage"],
            "finite-exec",
            None,
            [
                "input_tokens",
                "cached_input_tokens",
                "output_tokens",
                "reasoning_output_tokens",
                "total_tokens",
            ],
        ),
        _ => return None,
    };
    let mut result = json!({"session":session,"turn":turn,"scope":"session-total",
        "source":"provider-reported","cache_is_input_subset":true,"reasoning_is_output_subset":true});
    for (target, source) in [
        "input_tokens",
        "cached_input_tokens",
        "output_tokens",
        "reasoning_output_tokens",
        "total_tokens",
    ]
    .into_iter()
    .zip(names)
    {
        result[target] = usage[source].as_u64().map_or(Value::Null, |n| json!(n));
    }
    if result["total_tokens"].is_null() {
        result["total_tokens"] = result["input_tokens"]
            .as_u64()
            .zip(result["output_tokens"].as_u64())
            .and_then(|(a, b)| a.checked_add(b))
            .map_or(Value::Null, |n| json!(n));
    }
    Some(result)
}
