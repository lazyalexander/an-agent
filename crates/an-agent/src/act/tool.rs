use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use thiserror::Error;

use super::sense::effect_from_sentence;
use super::sentence::ActSentence;
use super::tag::{Permit, ToolTag};
use super::{ActEnvelope, ActKind};
use crate::memstream::{ActOnEvent, AppendEvent, FromKind, JsonlStore, Kind, Memevent, StoreError};

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("store: {0}")]
    Store(#[from] StoreError),
}

pub struct ToolCtx {
    pub signal: Option<tokio::sync::watch::Receiver<bool>>,
}

/// Carrying context for tape-emitting act paths: where events go, who acts,
/// and under which card (None for uncarded call sites).
pub struct ActCtx<'a> {
    pub store: Option<&'a JsonlStore>,
    pub agent_id: &'a str,
    pub session: &'a str,
    pub card: Option<&'a str>,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> Value;
    /// Optional seed for the act wrapper. Not part of the tool identity.
    fn tag_seed(&self) -> Option<ToolTag> {
        None
    }
    async fn execute(&self, args: Value, ctx: &ToolCtx) -> Result<String, String>;
}

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone)]
pub struct ToolMessage {
    pub tool_call_id: String,
    pub content: String,
}

pub fn tag_of(tool: Option<&dyn Tool>) -> ToolTag {
    tool.and_then(Tool::tag_seed).unwrap_or_else(ToolTag::unbounded)
}

pub struct ToolActResult {
    pub action: Option<Memevent>,
    pub observation: Option<Memevent>,
    pub message: ToolMessage,
}

fn parse_args(raw: &str) -> Result<Value, String> {
    let value: Value = serde_json::from_str(raw).map_err(|_| "tool arguments are not valid JSON".to_string())?;
    if !value.is_object() {
        return Err("tool arguments must be a JSON object".into());
    }
    Ok(value)
}

async fn run_body(tool: &dyn Tool, args: Value, ctx: &ToolCtx) -> String {
    match tool.execute(args, ctx).await {
        Ok(s) => s,
        Err(e) => e,
    }
}

pub async fn run_tool_act(
    actx: &ActCtx<'_>,
    tools: &[Arc<dyn Tool>],
    call: &ToolCall,
    ctx: &ToolCtx,
) -> Result<ToolActResult, ToolError> {
    let tool = tools.iter().find(|t| t.name() == call.name);
    let tag = tag_of(tool.map(|t| t.as_ref()));
    let sentence = match ActSentence::from_seed(&tag, None) {
        Ok(s) => s,
        Err(e) => {
            let env = ActEnvelope {
                kind: ActKind::Tool,
                sentence: ActSentence::bare(
                    tag.permit,
                    super::sentence::BareFile::None,
                    super::sentence::Ingest::Ignore,
                ),
                tool: Some(call.name.clone()),
            };
            let action = admit(
                actx,
                Kind::Action,
                format!("{} {}", call.name, call.arguments),
                vec![],
                Some(ActOnEvent::intent(&env)),
            )?;
            let refs = action.as_ref().map(|a| vec![a.id.clone()]).unwrap_or_default();
            let observation = admit(
                actx,
                Kind::Observation,
                e.to_string(),
                refs,
                Some(ActOnEvent::with_effect(&env, effect_from_sentence(&env.sentence))),
            )?;
            return Ok(ToolActResult {
                action,
                observation,
                message: ToolMessage {
                    tool_call_id: call.id.clone(),
                    content: e.to_string(),
                },
            });
        }
    };
    let env = ActEnvelope {
        kind: ActKind::Tool,
        sentence,
        tool: Some(call.name.clone()),
    };

    let action = admit(
        actx,
        Kind::Action,
        format!("{} {}", call.name, call.arguments),
        vec![],
        Some(ActOnEvent::intent(&env)),
    )?;

    let fail = |content: String, action: Option<Memevent>| -> Result<ToolActResult, ToolError> {
        let refs = action.as_ref().map(|a| vec![a.id.clone()]).unwrap_or_default();
        let observation = admit(
            actx,
            Kind::Observation,
            content.clone(),
            refs,
            Some(ActOnEvent::with_effect(&env, effect_from_sentence(&env.sentence))),
        )?;
        Ok(ToolActResult {
            action,
            observation,
            message: ToolMessage {
                tool_call_id: call.id.clone(),
                content,
            },
        })
    };

    let Some(tool) = tool else {
        return fail(format!("unknown tool: {}", call.name), action);
    };
    if env.sentence.permit() == Permit::Forbidden {
        return fail("forbidden".into(), action);
    }
    let args = match parse_args(&call.arguments) {
        Ok(v) => v,
        Err(e) => return fail(e, action),
    };
    let content = run_body(tool.as_ref(), args, ctx).await;
    fail(content, action)
}

fn admit(
    actx: &ActCtx<'_>,
    kind: Kind,
    content: String,
    refs: Vec<String>,
    act: Option<ActOnEvent>,
) -> Result<Option<Memevent>, ToolError> {
    let Some(store) = actx.store else {
        return Ok(None);
    };
    Ok(Some(store.append(AppendEvent {
        from: actx.agent_id.into(),
        from_kind: FromKind::Agent,
        kind,
        session: actx.session.into(),
        content,
        tags: vec![],
        refs,
        act,
        card: actx.card.map(str::to_string),
    })?))
}
