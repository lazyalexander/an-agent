//! Pinned app-server dialogue; all host effects reuse task capability checks.
use super::*;
mod continuation;
mod steering;
use std::{
    io::{Read, Write},
    process::{ChildStdin, ChildStdout},
    sync::{Arc, atomic::Ordering},
};

pub(super) fn execute(
    root: &Path,
    dir: &Path,
    task: &harness::Task,
    authority: &Binding,
    cancelled: &AtomicBool,
) -> Result<Value> {
    let settings = json!({"approval_policy":"never","project_doc_max_bytes":0,"web_search":"disabled",
        "default_permissions":"finite","permissions":{"finite":{"filesystem":{":root":"none",":project_roots":"none"},"network":{"enabled":false}}},
        "features":{"shell_tool":false,"view_image":false,"multi_agent":false,"apps":false,"plugins":false,
        "remote_plugin":false,"hooks":false,"goals":false,"skill_search":false,"skip_host_skill_discovery":true,
        "multi_agent_v2":{"enabled":false,"disable_native_tools":true}}});
    let mut args = vec!["app-server".into()];
    for (key, value) in settings.as_object().ok_or("settings")? {
        args.extend(["-c".into(), format!("{key}={}", super::codex::toml(value))]);
    }
    let result = crate::execution::sandbox::service_process::app_server(
        task,
        dir,
        &args,
        cancelled,
        |input, output, exceeded, stopped| {
            dialogue(
                root, dir, task, authority, cancelled, input, output, exceeded, stopped,
            )
        },
    )?;
    store::write_new(&dir.join("diagnostic.txt"), &result.stderr)?;
    let settlement = crate::execution::sandbox::workspace::settlement(result.pid, result.reaped);
    let confirmed = (result.pid.is_none() && result.status == "cancelled")
        || (result.reaped && settlement["empty"] == true);
    Ok(
        json!({"status":result.status,"termination_confirmed":confirmed,"group_observation":settlement,
        "proposal_available":result.status=="succeeded" && confirmed && dir.join("decision.json").is_file(),
        "artifact_accepted":false,"remote_termination_confirmed":false,"automatic_retries":0}),
    )
}

fn send(input: &mut ChildStdin, value: Value) -> std::io::Result<()> {
    serde_json::to_writer(&mut *input, &value)?;
    input.write_all(b"\n")?;
    input.flush()
}

#[allow(
    clippy::too_many_arguments,
    reason = "explicit original owner and transport bindings"
)]
fn dialogue(
    root: &Path,
    dir: &Path,
    task: &harness::Task,
    authority: &Binding,
    cancelled: &AtomicBool,
    mut input: ChildStdin,
    output: ChildStdout,
    exceeded: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
) -> std::io::Result<Vec<u8>> {
    let convert = |error: Box<dyn std::error::Error>| std::io::Error::other(error.to_string());
    let mut journal = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dir.join("native-events.jsonl"))?;
    send(
        &mut input,
        json!({"id":0,"method":"initialize","params":{"clientInfo":{"name":"an_agent","version":"0.1.0"},"capabilities":{"experimentalApi":true}}}),
    )?;
    store::directory(&dir.join("commands")).map_err(convert)?;
    let mut messages = steering::Messages::new(output);
    let mut thread = String::new();
    let mut turn = String::new();
    let mut calls = std::collections::BTreeSet::new();
    let mut total = 0;
    let mut completed = 0u8;
    loop {
        if dir.join("cancel.json").exists() || crate::task_permissions::active(authority).is_err() {
            cancelled.store(true, Ordering::Release);
            return Err(std::io::Error::other("task authorization cancelled"));
        }
        steering::send_pending(dir, authority, cancelled, &mut input, &thread, &turn)
            .map_err(convert)?;
        if turn.is_empty() && completed > 0 {
            if super::task_session::finish::ready(dir, completed).map_err(convert)? {
                let candidate =
                    store::read(&dir.join(format!("candidate-{completed}.json")), 131072)
                        .map_err(convert)?;
                store::write_new(&dir.join("decision.json"), &candidate).map_err(convert)?;
                return Ok(Vec::new());
            }
            continuation::send_pending(
                dir,
                &mut input,
                &thread,
                completed,
                &task.schema,
                authority,
                cancelled,
            )
            .map_err(convert)?;
        }
        let Some(raw) = messages.next()? else {
            continue;
        };
        let mut message: Value = serde_json::from_slice(&raw)?;
        message["host_observed_ms"] = json!(super::harness_metrics::now_ms());
        let record = serde_json::to_vec(&message)?;
        total += record.len() + 1;
        if total > 2 * 1024 * 1024 {
            exceeded.store(true, Ordering::Release);
            return Err(std::io::Error::other("app-server evidence limit"));
        }
        journal.write_all(&record)?;
        journal.write_all(b"\n")?;
        if let Some(next) = continuation::acknowledge(dir, &message, completed).map_err(convert)? {
            if !turn.is_empty() {
                return Err(std::io::Error::other(
                    "continuation overlapped an active turn",
                ));
            }
            turn = next;
            super::task_session::publish(
                &dir.join(format!("turn-start-{}.json", completed + 1)),
                &json!({"thread":thread,"turn":turn}),
            )
            .map_err(convert)?;
            calls.clear();
            continue;
        }
        if steering::acknowledge(dir, &message, &turn).map_err(convert)? {
            continue;
        }
        if message.get("error").is_some() {
            return Err(std::io::Error::other(
                "app-server request refused; inspect private events",
            ));
        }
        match message["id"].as_u64() {
            Some(0) if message.get("result").is_some() => {
                send(&mut input, json!({"method":"initialized"}))?;
                send(
                    &mut input,
                    json!({"id":1,"method":"thread/start","params":{"ephemeral":true,"cwd":dir.join("work"),
                    "model":task.model,"approvalPolicy":"never","permissions":"finite",
                    "dynamicTools":[{"type":"function","name":"host_request","description":"Call an-agent task tools. request is a JSON string. Mutations preview first, then use the returned approve digest. Use only granted resources. An optional grant selects an operator-issued descendant for delegation.",
                    "inputSchema":{"type":"object","properties":{"request":{"type":"string"}},"required":["request"],"additionalProperties":false}}]}}),
                )?;
            }
            Some(1) if message.get("result").is_some() => {
                thread = text(&message["result"]["thread"], "id")
                    .map_err(convert)?
                    .into();
                send(
                    &mut input,
                    json!({"id":2,"method":"turn/start","params":{"threadId":thread,
                    "input":[{"type":"text","text":task.prompt}],"outputSchema":task.schema}}),
                )?;
            }
            Some(2) if message.get("result").is_some() => {
                turn = text(&message["result"]["turn"], "id")
                    .map_err(convert)?
                    .into();
                let execution = store::digest(&serde_json::to_vec(&json!({
                    "owner":read(&dir.join("owner.json")).map_err(convert)?,"thread":thread,"turn":turn}))?);
                super::task_session::publish(
                    &dir.join("session.json"),
                    &json!({"execution":execution,"thread":thread,"turn":turn}),
                )
                .map_err(convert)?;
                super::task_session::publish(
                    &dir.join("turn-start-1.json"),
                    &json!({"thread":thread,"turn":turn}),
                )
                .map_err(convert)?;
            }
            _ => {
                if message.get("id").is_some() && message.get("method").is_some() {
                    let p = &message["params"];
                    if message["method"] != "item/tool/call"
                        || p["threadId"] != thread
                        || p["turnId"] != turn
                        || p["tool"] != "host_request"
                        || p["namespace"].as_str().is_some()
                        || !calls.insert(text(p, "callId").map_err(convert)?.to_owned())
                    {
                        return Err(std::io::Error::other(
                            "unbound, duplicate or unsupported app-server request",
                        ));
                    }
                    let id = dir
                        .file_name()
                        .and_then(|n| n.to_str())
                        .ok_or_else(|| std::io::Error::other("harness task ID"))?;
                    let result = super::task_session::invoke(
                        root,
                        &json!({"op":"harness-tool","id":id,"arguments":p["arguments"]}),
                        &stopped,
                    );
                    let success = result.is_ok();
                    let content = result.unwrap_or_else(|e| json!({"error":e.to_string()}));
                    send(
                        &mut input,
                        json!({"id":message["id"],"result":{"success":success,"contentItems":[{"type":"inputText","text":content.to_string()}]}}),
                    )?;
                } else if message["method"] == "item/completed"
                    && message["params"]["item"]["type"] == "agentMessage"
                    && message["params"]["item"]["phase"] != "commentary"
                {
                    if message["params"]["threadId"] != thread
                        || message["params"]["turnId"] != turn
                    {
                        return Err(std::io::Error::other("foreign agent result"));
                    }
                    if let Some(text) = message["params"]["item"]["text"].as_str() {
                        // Only the final structured message is published after completion.
                        let parsed: Value = serde_json::from_str(text)?;
                        store::write_new(
                            &dir.join(format!("candidate-{}.json", completed + 1)),
                            &serde_json::to_vec(&parsed)?,
                        )
                        .map_err(convert)?;
                    }
                } else if message["method"] == "turn/completed" {
                    if message["params"]["threadId"] != thread
                        || message["params"]["turn"]["id"] != turn
                        || message["params"]["turn"]["status"] != "completed"
                    {
                        return Err(std::io::Error::other("app-server turn did not complete"));
                    }
                    if crate::task_permissions::active(authority).is_err() {
                        cancelled.store(true, Ordering::Release);
                        return Err(std::io::Error::other("task authorization cancelled"));
                    }
                    completed += 1;
                    let candidate =
                        store::read(&dir.join(format!("candidate-{completed}.json")), 131072)
                            .map_err(convert)?;
                    super::task_session::publish(
                        &dir.join(format!("turn-result-{completed}.json")),
                        &json!({"thread":thread,"turn":turn,"number":completed,
                            "proposal":serde_json::from_slice::<Value>(&candidate)?,
                            "evidence":"provider-completed-turn","artifact_accepted":false,
                            "termination_confirmed":false}),
                    )
                    .map_err(convert)?;
                    journal.sync_all()?;
                    if completed == task.turn_limit.unwrap_or(1) {
                        store::write_new(&dir.join("decision.json"), &candidate)
                            .map_err(convert)?;
                        return Ok(Vec::new()); // Closing stdin lets the app-server shut down.
                    }
                    turn.clear();
                }
            }
        }
    }
}
