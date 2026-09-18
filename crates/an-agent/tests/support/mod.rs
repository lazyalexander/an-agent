//! Probe scaffolding shared by integration tests: the sequential ReAct loop
//! (one policy, not a kernel contract) and the chat-completions wire/client.
//! Formerly src/agent.rs and src/model.rs — moved out of the kernel so
//! probe-grade code cannot be mistaken for, or depended on by, formal code.
//! Src must never depend on this; deletion and rewrite are expected.

// clippy.toml's allow-*-in-tests covers #[test] fns only; this support module
// is ordinary code inside test crates, so it carries its own allowance.
#![allow(clippy::unwrap_used, clippy::expect_used)]

pub mod rhai;
pub mod ts_tool;

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use an_agent::act::{
    ActEnvelope, ActKind, ActSentence, BareFile, Ingest, Permit, Tool, ToolCall,
    effect_from_sentence,
};
use an_agent::memstream::{ActOnEvent, AppendEvent, FromKind, Kind, Memevent};

// --- wire types (OpenAI-style chat completions; vendor detail) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<WireToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub function: WireFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone)]
pub struct Assistant {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub usage: Option<Usage>,
}

/// Token accounting for one model call; None when the wire omits it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("store: {0}")]
    Store(#[from] an_agent::memstream::StoreError),
    #[error("tool: {0}")]
    Tool(#[from] an_agent::act::ToolError),
    #[error("model: {0}")]
    Model(String),
}

pub trait Model: Send + Sync {
    /// Self-reported spec, taped on every invoke intent (thin trace, T2-style).
    fn spec(&self) -> serde_json::Value {
        serde_json::Value::Null
    }
    fn complete(
        &self,
        messages: &[ChatMessage],
        tools: &[Arc<dyn Tool>],
    ) -> impl std::future::Future<Output = Result<Assistant, AgentError>> + Send;
}

// --- the sequential ReAct loop (one policy) ---

pub struct AgentState {
    pub messages: Vec<ChatMessage>,
}

pub async fn step(
    state: AgentState,
    actx: &an_agent::act::ActCtx<'_>,
    model: &impl Model,
    tools: &[Arc<dyn Tool>],
    ctx: &an_agent::act::ToolCtx,
) -> Result<AgentState, AgentError> {
    let invoke = admit_invoke_intent(actx, model, &state.messages)?;
    let assistant = model.complete(&state.messages, tools).await?;
    admit_invoke_effect(actx, invoke.as_ref(), &assistant)?;
    if !assistant.content.is_empty() {
        let _ = admit_utterance(actx, &assistant.content)?;
    }
    let mut messages = state.messages;
    let tool_calls = assistant.tool_calls.clone();
    messages.push(ChatMessage {
        role: "assistant".into(),
        content: if assistant.content.is_empty() {
            None
        } else {
            Some(assistant.content)
        },
        tool_calls: if tool_calls.is_empty() {
            None
        } else {
            Some(
                tool_calls
                    .iter()
                    .map(|c| WireToolCall {
                        id: c.id.clone(),
                        type_: "function".into(),
                        function: WireFunction {
                            name: c.name.clone(),
                            arguments: c.arguments.clone(),
                        },
                    })
                    .collect(),
            )
        },
        tool_call_id: None,
    });
    if tool_calls.is_empty() {
        return Ok(AgentState { messages });
    }
    for call in &tool_calls {
        let result = an_agent::act::run_tool_act(actx, tools, call, ctx).await?;
        messages.push(ChatMessage {
            role: "tool".into(),
            content: Some(result.message.content),
            tool_calls: None,
            tool_call_id: Some(result.message.tool_call_id),
        });
    }
    Ok(AgentState { messages })
}

pub async fn run_until_idle(
    mut state: AgentState,
    actx: &an_agent::act::ActCtx<'_>,
    model: &impl Model,
    tools: &[Arc<dyn Tool>],
    ctx: &an_agent::act::ToolCtx,
    max_steps: u32,
) -> Result<AgentState, AgentError> {
    for _ in 0..max_steps {
        state = step(state, actx, model, tools, ctx).await?;
        if last_is_final_assistant(&state) {
            return Ok(state);
        }
    }
    Err(AgentError::Model("agent exceeded maxSteps".into()))
}

fn last_is_final_assistant(state: &AgentState) -> bool {
    let Some(last) = state.messages.last() else {
        return false;
    };
    last.role == "assistant"
        && last
            .tool_calls
            .as_ref()
            .map(|c| c.is_empty())
            .unwrap_or(true)
}

fn admit_utterance(
    actx: &an_agent::act::ActCtx<'_>,
    content: &str,
) -> Result<Option<Memevent>, AgentError> {
    let Some(store) = actx.store else {
        return Ok(None);
    };
    let env = ActEnvelope {
        kind: ActKind::Utterance,
        sentence: ActSentence::bare(Permit::Go, BareFile::None, Ingest::Ignore),
        tool: None,
    };
    Ok(Some(store.append(AppendEvent {
        from: actx.agent_id.into(),
        from_kind: FromKind::Agent,
        kind: Kind::Utterance,
        session: actx.session.into(),
        content: content.into(),
        tags: vec![],
        refs: vec![],
        act: Some(ActOnEvent::intent(&env)),
        card: actx.card.map(str::to_string),
    })?))
}

fn invoke_envelope() -> ActEnvelope {
    ActEnvelope {
        kind: ActKind::Invoke,
        sentence: ActSentence::bare(Permit::Go, BareFile::None, Ingest::Ignore),
        tool: None,
    }
}

fn sha256_hex(bytes: impl AsRef<[u8]>) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes.as_ref())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Tape the model call itself: intent carries spec + input hash (thin trace,
/// no full prompt), effect carries response hash + tool-call names + usage.
/// Skipped when no store is attached.
fn admit_invoke_intent(
    actx: &an_agent::act::ActCtx<'_>,
    model: &impl Model,
    messages: &[ChatMessage],
) -> Result<Option<Memevent>, AgentError> {
    let Some(store) = actx.store else {
        return Ok(None);
    };
    let env = invoke_envelope();
    let content = serde_json::json!({
        "spec": model.spec(),
        "messages_hash": sha256_hex(serde_json::to_vec(messages).unwrap_or_default()),
    });
    Ok(Some(store.append(AppendEvent {
        from: actx.agent_id.into(),
        from_kind: FromKind::Agent,
        kind: Kind::Action,
        session: actx.session.into(),
        content: content.to_string(),
        tags: vec![],
        refs: vec![],
        act: Some(ActOnEvent::intent(&env)),
        card: actx.card.map(str::to_string),
    })?))
}

fn admit_invoke_effect(
    actx: &an_agent::act::ActCtx<'_>,
    intent: Option<&Memevent>,
    assistant: &Assistant,
) -> Result<Option<Memevent>, AgentError> {
    let Some(store) = actx.store else {
        return Ok(None);
    };
    let env = invoke_envelope();
    let content = serde_json::json!({
        "content_hash": sha256_hex(assistant.content.as_bytes()),
        "tool_calls": assistant.tool_calls.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        "usage": assistant.usage.map(|u| serde_json::json!({
            "prompt_tokens": u.prompt_tokens,
            "completion_tokens": u.completion_tokens,
        })),
    });
    let refs = intent.map(|e| vec![e.id.clone()]).unwrap_or_default();
    Ok(Some(store.append(AppendEvent {
        from: actx.agent_id.into(),
        from_kind: FromKind::Agent,
        kind: Kind::Observation,
        session: actx.session.into(),
        content: content.to_string(),
        tags: vec![],
        refs,
        act: Some(ActOnEvent::with_effect(
            &env,
            effect_from_sentence(&env.sentence),
        )),
        card: actx.card.map(str::to_string),
    })?))
}

pub fn last_assistant_text(state: &AgentState) -> String {
    state
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "assistant")
        .and_then(|m| m.content.clone())
        .unwrap_or_default()
}

// --- policy-driven loop (control flow lives in a rhai script) ---
//
// The fixed ReAct policy above is one hardcoded policy; here the policy is
// a rhai script mounted on tape. The script sees a tape projection (clip
// ids + previews, not full text) and yields a continuation; the host
// admits and executes it. The loop stays in the host.

use self::rhai::{Continuation, RhaiPolicy};
use an_agent::memstream::JsonlStore;

/// How many recent events the policy sees, newest first.
const PROJECTION_TAIL: usize = 20;
const CLIP_PREVIEW_CHARS: usize = 200;

fn ulid() -> String {
    an_agent::det_seam::Entropy::os()
        .ulid(an_agent::det_seam::Clock::wall().now_ms())
        .to_string()
}

/// Tape the policy descriptor itself (script inline, content-addressed):
/// the audit/reuse source of the control flow about to run.
pub fn mount_policy(
    actx: &an_agent::act::ActCtx<'_>,
    name: &str,
    yaml: &str,
) -> Result<Option<String>, AgentError> {
    let hash = an_agent::tools::descriptor::content_hash(yaml);
    let Some(store) = actx.store else {
        return Ok(None);
    };
    let env = ActEnvelope {
        kind: ActKind::Mount,
        sentence: ActSentence::bare(Permit::Go, BareFile::None, Ingest::Ignore),
        tool: Some(name.to_string()),
    };
    store.append(AppendEvent {
        from: actx.agent_id.into(),
        from_kind: FromKind::Agent,
        kind: Kind::Action,
        session: actx.session.into(),
        content: yaml.to_string(),
        tags: vec!["policy".into(), hash.clone()],
        refs: vec![],
        act: Some(ActOnEvent::intent(&env)),
        card: actx.card.map(str::to_string),
    })?;
    Ok(Some(hash))
}

/// Clip ids + metadata + truncated preview, newest first — not full text.
/// The script pulls full content only indirectly, by naming clip ids in
/// `invoke_model` and letting the host assemble the context.
fn tape_projection(store: &JsonlStore, session: &str) -> Result<serde_json::Value, AgentError> {
    let events = store.read_all()?;
    let clips: Vec<serde_json::Value> = events
        .iter()
        .rev()
        .filter(|e| e.session.as_deref() == Some(session))
        .take(PROJECTION_TAIL)
        .map(|e| {
            serde_json::json!({
                "id": e.id,
                "kind": e.kind,
                "from": e.from_kind,
                "tags": e.tags,
                "preview": e.content.chars().take(CLIP_PREVIEW_CHARS).collect::<String>(),
            })
        })
        .collect();
    Ok(serde_json::Value::Array(clips))
}

fn last_observation_preview(store: &JsonlStore, session: &str) -> Result<String, AgentError> {
    Ok(store
        .read_all()?
        .iter()
        .rev()
        .find(|e| e.kind == Kind::Observation && e.session.as_deref() == Some(session))
        .map(|e| e.content.chars().take(CLIP_PREVIEW_CHARS).collect())
        .unwrap_or_default())
}

/// One policy step: project the tape, run the script through admission
/// (the decision itself is taped as intent/effect), execute the
/// continuation. Returns Ok(true) on halt.
pub async fn policy_step(
    actx: &an_agent::act::ActCtx<'_>,
    model: &impl Model,
    policy: &Arc<RhaiPolicy>,
    tools: &[Arc<dyn Tool>],
    ctx: &an_agent::act::ToolCtx,
) -> Result<bool, AgentError> {
    let (clips, last_obs) = match actx.store {
        Some(store) => (
            tape_projection(store, actx.session)?,
            last_observation_preview(store, actx.session)?,
        ),
        None => (serde_json::json!([]), String::new()),
    };
    let args = serde_json::json!({ "clips": clips, "last_observation": last_obs });
    let call = ToolCall {
        id: ulid(),
        name: policy.name().to_string(),
        arguments: args.to_string(),
    };
    let policy_slot = [policy.clone() as Arc<dyn Tool>];
    let decision = an_agent::act::run_tool_act(actx, &policy_slot, &call, ctx).await?;
    let value: serde_json::Value = serde_json::from_str(&decision.message.content)
        .map_err(|e| AgentError::Model(format!("policy output is not a continuation: {e}")))?;
    let cont = Continuation::from_value(&value).map_err(AgentError::Model)?;
    match cont {
        Continuation::Halt => Ok(true),
        Continuation::Utter { text } => {
            let _ = admit_utterance(actx, &text)?;
            Ok(false)
        }
        Continuation::InvokeModel { clips } => {
            let store = actx
                .store
                .ok_or_else(|| AgentError::Model("invoke_model needs a store".into()))?;
            // The host assembles context from the named clip ids; the
            // script never touches full text or the model itself.
            let events = store.read_all()?;
            let mut messages = Vec::with_capacity(clips.len());
            for id in &clips {
                let e = events
                    .iter()
                    .find(|e| &e.id == id)
                    .ok_or_else(|| AgentError::Model(format!("unknown clip id: {id}")))?;
                messages.push(ChatMessage {
                    role: match e.from_kind {
                        FromKind::Agent => "assistant",
                        _ => "user",
                    }
                    .into(),
                    content: Some(e.content.clone()),
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
            let invoke = admit_invoke_intent(actx, model, &messages)?;
            // The policy owns control flow: the model is an oracle called
            // without tools; any tool_calls it emits are ignored.
            let assistant = model.complete(&messages, &[]).await?;
            admit_invoke_effect(actx, invoke.as_ref(), &assistant)?;
            if !assistant.content.is_empty() {
                let _ = admit_utterance(actx, &assistant.content)?;
            }
            Ok(false)
        }
        Continuation::InvokeTool { name, args } => {
            // Static composition: a policy may only yield tools named in
            // its descriptor's requires; anything else fails hard (the
            // decision is already on tape, the call never happens).
            if !policy.whitelist().iter().any(|n| n == &name) {
                return Err(AgentError::Model(format!(
                    "policy yielded tool outside its requires: {name}"
                )));
            }
            let call = ToolCall {
                id: ulid(),
                name,
                arguments: args.to_string(),
            };
            let _ = an_agent::act::run_tool_act(actx, tools, &call, ctx).await?;
            Ok(false)
        }
    }
}

pub async fn run_policy_until_idle(
    actx: &an_agent::act::ActCtx<'_>,
    model: &impl Model,
    policy: &Arc<RhaiPolicy>,
    tools: &[Arc<dyn Tool>],
    ctx: &an_agent::act::ToolCtx,
    max_steps: u32,
) -> Result<(), AgentError> {
    for _ in 0..max_steps {
        if policy_step(actx, model, policy, tools, ctx).await? {
            return Ok(());
        }
    }
    Err(AgentError::Model("policy exceeded maxSteps".into()))
}

// --- chat-completions HTTP client (live probes only) ---

#[derive(Debug, Clone)]
pub struct ModelSettings {
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    pub extra_body: Option<serde_json::Value>,
}

#[derive(Clone)]
pub struct ChatCompletions {
    client: reqwest::Client,
    settings: ModelSettings,
}

impl ChatCompletions {
    pub fn new(settings: ModelSettings) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(180))
                .build()
                .expect("reqwest client"),
            settings,
        }
    }
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    usage: Option<WireUsage>,
}

#[derive(Deserialize)]
struct WireUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
}

#[derive(Deserialize)]
struct Choice {
    message: WireAssistant,
}

#[derive(Deserialize)]
struct WireAssistant {
    content: Option<String>,
    tool_calls: Option<Vec<WireToolCall>>,
}

impl Model for ChatCompletions {
    fn spec(&self) -> serde_json::Value {
        serde_json::json!({
            "base_url": self.settings.base_url,
            "model": self.settings.model,
            "extra_body": self.settings.extra_body,
        })
    }

    async fn complete(
        &self,
        messages: &[ChatMessage],
        tools: &[Arc<dyn Tool>],
    ) -> Result<Assistant, AgentError> {
        let url = format!(
            "{}/chat/completions",
            self.settings.base_url.trim_end_matches('/')
        );
        let mut body = serde_json::json!({
            "model": self.settings.model,
            "messages": messages,
        });
        if !tools.is_empty() {
            let listed: Vec<serde_json::Value> = tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": t.name(),
                            "description": t.description(),
                            "parameters": t.parameters(),
                        }
                    })
                })
                .collect();
            body["tools"] = serde_json::Value::Array(listed);
        }
        if let Some(extra) = &self.settings.extra_body
            && let (Some(base), Some(over)) = (body.as_object_mut(), extra.as_object())
        {
            for (k, v) in over {
                base.insert(k.clone(), v.clone());
            }
        }
        let mut req = self.client.post(url).json(&body);
        if let Some(key) = &self.settings.api_key {
            req = req.bearer_auth(key);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| AgentError::Model(e.to_string()))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(AgentError::Model(format!("{status}: {text}")));
        }
        let parsed: ChatResponse = resp
            .json()
            .await
            .map_err(|e| AgentError::Model(e.to_string()))?;
        let msg = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| AgentError::Model("empty choices".into()))?
            .message;
        let tool_calls = msg
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .map(|c| ToolCall {
                id: if c.id.is_empty() {
                    an_agent::det_seam::Entropy::os()
                        .ulid(an_agent::det_seam::Clock::wall().now_ms())
                        .to_string()
                } else {
                    c.id
                },
                name: c.function.name,
                arguments: c.function.arguments,
            })
            .collect();
        Ok(Assistant {
            content: msg.content.unwrap_or_default(),
            tool_calls,
            usage: parsed.usage.map(|u| Usage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
            }),
        })
    }
}

// --- shared temp dir guard (integration tests cannot use src testkit) ---

pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "an-agent-test-{tag}-{}",
            an_agent::det_seam::Entropy::os().ulid(an_agent::det_seam::Clock::wall().now_ms())
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        TempDir(dir)
    }

    pub fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
