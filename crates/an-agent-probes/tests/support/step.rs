//! Sequential loop. One model call, then the tool calls it named.

use super::*;

pub async fn step(
    state: AgentState,
    actx: &an_agent_core::act::ActCtx<'_>,
    model: &impl Model,
    tools: &[Arc<dyn Tool>],
    ctx: &an_agent_core::act::ToolCtx,
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
        let result = an_agent_core::act::run_tool_act(actx, tools, call, ctx).await?;
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
    actx: &an_agent_core::act::ActCtx<'_>,
    model: &impl Model,
    tools: &[Arc<dyn Tool>],
    ctx: &an_agent_core::act::ToolCtx,
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

pub(super) fn admit_utterance(
    actx: &an_agent_core::act::ActCtx<'_>,
    content: &str,
) -> Result<Option<Memevent>, AgentError> {
    let Some(store) = actx.store else {
        return Ok(None);
    };
    let env = ActEnvelope {
        kind: ActKind::Utterance,
        permit: Permit::Go,
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
        permit: Permit::Go,
        tool: None,
    }
}

/// Tape the model call itself: intent carries spec + input hash (thin trace,
/// no full prompt), effect carries response hash + tool-call names + usage.
/// Skipped when no store is attached.
pub(super) fn admit_invoke_intent(
    actx: &an_agent_core::act::ActCtx<'_>,
    model: &impl Model,
    messages: &[ChatMessage],
) -> Result<Option<Memevent>, AgentError> {
    let Some(store) = actx.store else {
        return Ok(None);
    };
    let env = invoke_envelope();
    let content = serde_json::json!({
        "spec": model.spec(),
        "messages_hash": super::sha256_hex(serde_json::to_vec(messages).unwrap_or_default()),
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

pub(super) fn admit_invoke_effect(
    actx: &an_agent_core::act::ActCtx<'_>,
    intent: Option<&Memevent>,
    assistant: &Assistant,
) -> Result<Option<Memevent>, AgentError> {
    let Some(store) = actx.store else {
        return Ok(None);
    };
    let env = invoke_envelope();
    let content = serde_json::json!({
        "content_hash": super::sha256_hex(assistant.content.as_bytes()),
        // Full proposals, args included: a policy approves by reference
        // (clip + index) and never copies arguments through the script.
        "tool_calls": assistant.tool_calls.iter().map(|c| serde_json::json!({
            "name": c.name,
            "arguments": c.arguments,
        })).collect::<Vec<_>>(),
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
            effect_from_tag(&ToolTag::none(), None),
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
