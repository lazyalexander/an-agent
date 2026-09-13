use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::act::{run_tool_act, ActEnvelope, ActKind, Tool, ToolCall, ToolCtx, ToolError, ToolTag};
use crate::memstream::{ActOnEvent, AppendEvent, FromKind, JsonlStore, Kind, StoreError};

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
}

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("store: {0}")]
    Store(#[from] StoreError),
    #[error("tool: {0}")]
    Tool(#[from] ToolError),
    #[error("model: {0}")]
    Model(String),
}

pub trait Model: Send + Sync {
    fn complete(
        &self,
        messages: &[ChatMessage],
        tools: &[Arc<dyn Tool>],
    ) -> impl std::future::Future<Output = Result<Assistant, AgentError>> + Send;
}

pub struct AgentState {
    pub messages: Vec<ChatMessage>,
}

pub async fn step(
    state: AgentState,
    store: Option<&JsonlStore>,
    agent_id: &str,
    session: &str,
    model: &impl Model,
    tools: &[Arc<dyn Tool>],
    ctx: &ToolCtx,
) -> Result<AgentState, AgentError> {
    let assistant = model.complete(&state.messages, tools).await?;
    if !assistant.content.is_empty() {
        let _ = admit_utterance(store, agent_id, session, &assistant.content)?;
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
        let result = run_tool_act(store, agent_id, session, tools, call, ctx).await?;
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
    store: Option<&JsonlStore>,
    agent_id: &str,
    session: &str,
    model: &impl Model,
    tools: &[Arc<dyn Tool>],
    ctx: &ToolCtx,
    max_steps: u32,
) -> Result<AgentState, AgentError> {
    for _ in 0..max_steps {
        state = step(state, store, agent_id, session, model, tools, ctx).await?;
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
    last.role == "assistant" && last.tool_calls.as_ref().map(|c| c.is_empty()).unwrap_or(true)
}

fn admit_utterance(
    store: Option<&JsonlStore>,
    agent_id: &str,
    session: &str,
    content: &str,
) -> Result<Option<crate::memstream::Memevent>, AgentError> {
    let Some(store) = store else {
        return Ok(None);
    };
    let env = ActEnvelope {
        kind: ActKind::Utterance,
        tag: ToolTag::none(),
        workplace: None,
        tool: None,
    };
    Ok(Some(store.append(AppendEvent {
        from: agent_id.into(),
        from_kind: FromKind::Agent,
        kind: Kind::Utterance,
        session: session.into(),
        content: content.into(),
        tags: vec![],
        refs: vec![],
        act: Some(ActOnEvent::intent(&env)),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::act::{Permit, ToolTag};
    use serde_json::json;

    struct Fake(Assistant);
    impl Model for Fake {
        async fn complete(
            &self,
            _messages: &[ChatMessage],
            _tools: &[Arc<dyn Tool>],
        ) -> Result<Assistant, AgentError> {
            Ok(self.0.clone())
        }
    }

    struct Echo;
    #[async_trait::async_trait]
    impl Tool for Echo {
        fn name(&self) -> &str {
            "echo"
        }
        fn description(&self) -> &str {
            "echo"
        }
        fn parameters(&self) -> serde_json::Value {
            json!({})
        }
        fn tag_seed(&self) -> Option<ToolTag> {
            Some(ToolTag::none())
        }
        async fn execute(&self, args: serde_json::Value, _ctx: &ToolCtx) -> Result<String, String> {
            Ok(args
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string())
        }
    }

    struct Locked;
    #[async_trait::async_trait]
    impl Tool for Locked {
        fn name(&self) -> &str {
            "echo"
        }
        fn description(&self) -> &str {
            "echo"
        }
        fn parameters(&self) -> serde_json::Value {
            json!({})
        }
        fn tag_seed(&self) -> Option<ToolTag> {
            Some(ToolTag::none_permit(Permit::Forbidden))
        }
        async fn execute(&self, _args: serde_json::Value, _ctx: &ToolCtx) -> Result<String, String> {
            panic!("must not execute");
        }
    }

    fn ctx() -> ToolCtx {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        ToolCtx { signal: Some(rx) }
    }

    #[tokio::test]
    async fn tool_observation_refs_action() {
        let dir = std::env::temp_dir().join(format!("an-agent-act-{}", ulid::Ulid::new()));
        let store = JsonlStore::open(dir.join("memory.jsonl")).unwrap();
        let model = Fake(Assistant {
            content: "using echo".into(),
            tool_calls: vec![ToolCall {
                id: "c1".into(),
                name: "echo".into(),
                arguments: r#"{"text":"z"}"#.into(),
            }],
        });
        let tools: Vec<Arc<dyn Tool>> = vec![Arc::new(Echo)];
        let state = AgentState {
            messages: vec![ChatMessage {
                role: "user".into(),
                content: Some("z".into()),
                tool_calls: None,
                tool_call_id: None,
            }],
        };
        let _ = step(state, Some(&store), "agent-1", "session-1", &model, &tools, &ctx())
            .await
            .unwrap();
        let events = store.read_all().unwrap();
        let action = events.iter().find(|e| e.kind == Kind::Action).unwrap();
        let obs = events.iter().find(|e| e.kind == Kind::Observation).unwrap();
        assert_eq!(obs.refs, vec![action.id.clone()]);
        assert_eq!(obs.content, "z");
        assert_eq!(action.act.as_ref().unwrap().tool.as_deref(), Some("echo"));
        assert!(obs.act.as_ref().unwrap().effect.is_some());
    }

    #[tokio::test]
    async fn forbidden_does_not_execute() {
        let model = Fake(Assistant {
            content: String::new(),
            tool_calls: vec![ToolCall {
                id: "c1".into(),
                name: "echo".into(),
                arguments: "{}".into(),
            }],
        });
        let tools: Vec<Arc<dyn Tool>> = vec![Arc::new(Locked)];
        let state = AgentState {
            messages: vec![ChatMessage {
                role: "user".into(),
                content: Some("x".into()),
                tool_calls: None,
                tool_call_id: None,
            }],
        };
        let next = step(state, None, "a", "s", &model, &tools, &ctx())
            .await
            .unwrap();
        assert_eq!(next.messages.last().unwrap().content.as_deref(), Some("forbidden"));
    }
}
