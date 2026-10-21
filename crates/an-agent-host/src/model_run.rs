//! One approved text request, using the existing admission and evidence path.
use crate::{
    ModelConnection, context_exchange,
    execution::{Result, store},
};
use an_agent_core::{
    control::{
        AgentBody, AgentControl, BeatStep, DeliveryOutcome, EventRoute, Registration,
        SpoolDescriptor,
    },
    principal::card::{AgentCard, ModelSpec, Topology},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{fs, path::Path, sync::Arc};
use uuid::Uuid;

const DESCRIPTOR: &str = include_str!("model_run.yaml");

/// Operator configuration, never supplied by the kit or a model response.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    base_url: String,
    model: String,
    system_prompt: String,
    auth_env: Option<String>,
    max_tokens: u32,
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u64,
}

fn default_timeout_ms() -> u64 {
    300_000
}

impl Profile {
    fn spec(&self) -> Result<ModelSpec> {
        if self.schema != "an-model-profile-v1"
            || self.system_prompt.len() > 4096
            || self.max_tokens == 0
            || self.max_tokens > 1024
            || self.timeout_ms == 0
            || self.timeout_ms > 300_000
            || self.auth_env.as_ref().is_some_and(|name| {
                name.is_empty()
                    || name.len() > 128
                    || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            })
        {
            return Err("invalid model profile".into());
        }
        Ok(ModelSpec {
            base_url: self.base_url.clone(),
            model: self.model.clone(),
            extra_body: Some(json!({"max_tokens":self.max_tokens})),
        })
    }
}

struct OneRequest {
    clip: String,
}
impl AgentBody for OneRequest {
    fn evaluate(&self, projection: &Value) -> std::result::Result<BeatStep, String> {
        if projection["steps"] == 0 {
            Ok(BeatStep::InvokeModel {
                clips: vec![self.clip.clone()],
            })
        } else {
            // The host copies the response blob, never the fused projection.
            Ok(BeatStep::Utter {
                text: "Model response ready for host persistence.".into(),
            })
        }
    }
}

/// Without an approval digest this only previews: no secret lookup, output
/// directory, model call or durable grant. The trusted operator supplies the
/// exact digest to authorize one request; this is not a remote auth protocol.
pub fn run(
    profile_path: &Path,
    packet_path: &Path,
    output: &Path,
    approval: Option<&str>,
) -> Result<Value> {
    run_cancellable(
        profile_path,
        packet_path,
        output,
        approval,
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
    )
}

/// The caller owns the cancellation flag. Interruptions settle evidence without retry.
pub fn run_cancellable(
    profile_path: &Path,
    packet_path: &Path,
    output: &Path,
    approval: Option<&str>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
) -> Result<Value> {
    let profile_bytes = store::read(profile_path, 16384)?;
    let profile: Profile = serde_json::from_slice(&profile_bytes)?;
    let spec = profile.spec()?;
    ModelConnection::new(spec.clone(), None)?;
    let packet_bytes = store::read(packet_path, 4096)?;
    let packet: context_exchange::ContextPacket = serde_json::from_slice(&packet_bytes)?;
    context_exchange::ContextPacket::parse(&packet_bytes, packet.audience)?;
    store::no_links(output)?;
    if output.exists() {
        return Err("model output already exists; never overwrite or retry implicitly".into());
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let output = parent
        .canonicalize()?
        .join(output.file_name().ok_or("missing output name")?);
    let plan = json!({"schema":"an-model-plan-v1", "profile_sha256":store::digest(&profile_bytes), "context_sha256":store::digest(&packet_bytes), "audience":packet.audience, "endpoint":spec.base_url, "model":spec.model, "max_tokens":profile.max_tokens, "timeout_ms":profile.timeout_ms, "auth_env":profile.auth_env, "output":output, "descriptor_sha256":store::digest(DESCRIPTOR.as_bytes()), "automatic_retries":0});
    let plan_bytes = serde_json::to_vec(&plan)?;
    let plan_sha = store::digest(&plan_bytes);
    let Some(approval) = approval else {
        return Ok(json!({"plan":plan,"approval_sha256":plan_sha,"executed":false}));
    };
    if approval != plan_sha {
        return Err("model approval digest mismatch; preview again".into());
    }
    let credential = profile
        .auth_env
        .as_ref()
        .map(|name| std::env::var(name).map_err(|_| "model credential is unavailable"))
        .transpose()?;
    let call = crate::ModelCallControl::new(
        cancelled,
        std::time::Duration::from_millis(profile.timeout_ms),
    )?;
    let connection = ModelConnection::new(spec.clone(), credential)?.with_control(call.clone());
    // create_dir is exclusive, including concurrent executions of the same plan.
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(&output)?;
    }
    #[cfg(not(unix))]
    fs::create_dir(&output)?;
    store::write_new(&output.join("plan.json"), &plan_bytes)?;
    store::write_new(&output.join("profile.json"), &profile_bytes)?;
    store::write_new(&output.join("context.json"), &packet_bytes)?;
    let control = AgentControl::open(output.join("agent"), &[])?;
    let result = invoke(
        &control,
        packet.audience,
        spec,
        &profile.system_prompt,
        connection,
        &packet_bytes,
        &output,
    );
    let (status, detail) = match result {
        Ok(detail) => ("succeeded", detail),
        Err(_) => (
            match call.status() {
                "cancelled" => "cancelled",
                "timed_out" => "timed_out",
                _ => "failed",
            },
            json!({"reason":"model request did not complete; inspect audit; do not retry automatically"}),
        ),
    };
    // Failed model calls already settle their own attempt on tape. Do not mark
    // output applied when transport/admission/persistence failed.
    let finished = control.finish(packet.audience, &format!("Model CLI settled: {status}"));
    let audit = control.events(packet.audience)?;
    store::write_new(
        &output.join("audit.json"),
        &serde_json::to_vec_pretty(&audit)?,
    )?;
    finished?;
    let remote_outcome = if call.status() == "succeeded" {
        "response_received"
    } else {
        "unknown"
    };
    let receipt = json!({"schema":"an-model-receipt-v1","status":status,"model_attempt":call.status(),"remote_outcome":remote_outcome,"plan_sha256":plan_sha,"thread":packet.audience,"result":detail,"retention":"manual","retry":"new explicit approval required; remote request may already have executed"});
    store::write_new(
        &output.join("receipt.json"),
        &serde_json::to_vec_pretty(&receipt)?,
    )?;
    Ok(
        json!({"status":status,"receipt":output.join("receipt.json"),"output": if status == "succeeded" {Some(output.join("output.txt"))} else {None}}),
    )
}

fn invoke(
    control: &AgentControl,
    id: Uuid,
    spec: ModelSpec,
    prompt: &str,
    connection: ModelConnection,
    packet: &[u8],
    output: &Path,
) -> Result<Value> {
    control.register(&Registration {
        events: vec![EventRoute {
            name: "model.request".into(),
            spools: vec!["host-model".into()],
        }],
        config: vec![],
        env: vec![],
    })?;
    control.set_model_client(Arc::new(connection));
    let thread = control.open_thread(&AgentCard {
        v: 1,
        id,
        model: spec,
        prompt: prompt.into(),
        tools: vec![],
        topology: Topology::Leaf,
        kernel: "0.1.0".into(),
        supersedes: None,
    })?;
    let clip = context_exchange::import(control, thread, packet)?;
    let descriptor = an_agent_spool::spool::parse(DESCRIPTOR)?;
    control.mount_agent_described(
        "host-model",
        Arc::new(OneRequest { clip }),
        &descriptor.declaration(),
        &SpoolDescriptor {
            package: descriptor.name,
            yaml: DESCRIPTOR.into(),
        },
    )?;
    let event = control.push_event("model.request", "Explicitly approved model request")?;
    let outcome = control.offer_on(thread, "host-model", &event.id)?;
    let DeliveryOutcome::Reply(reply) = outcome else {
        return Err("model admission refused".into());
    };
    let observation = control
        .events(thread)?
        .into_iter()
        .rev()
        .filter_map(|e| serde_json::from_str::<Value>(&e.content).ok())
        .find(|v| v.get("response_sha256").is_some())
        .ok_or("missing model response evidence")?;
    let sha = observation["response_sha256"]
        .as_str()
        .ok_or("missing response hash")?;
    if sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("invalid response reference".into());
    }
    let bytes = store::read(
        &control.directory(thread)?.join("blobs").join(sha),
        1024 * 1024,
    )?;
    if store::digest(&bytes) != sha {
        return Err("model response hash mismatch".into());
    }
    std::str::from_utf8(&bytes)?;
    store::write_new(&output.join("output.txt"), &bytes)?;
    control.applied(thread, &reply, "Model response artifact persisted")?;
    Ok(observation)
}
