//! Loop behavior tests for the probe ReAct loop (tests/support): utterance
//! admission, observation→action refs, and that a Forbidden permit never
//! reaches execution. Migrated from the former src/agent.rs unit tests.

#[allow(dead_code)]
mod support;

use std::sync::Arc;

use an_agent::act::{ActCtx, Permit, Tool, ToolCall, ToolCtx, ToolTag};
use an_agent::memstream::{JsonlStore, Kind};
use serde_json::json;
use support::{step, AgentError, AgentState, Assistant, ChatMessage, Model, TempDir};

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

fn one_user_msg(text: &str) -> AgentState {
    AgentState {
        messages: vec![ChatMessage {
            role: "user".into(),
            content: Some(text.into()),
            tool_calls: None,
            tool_call_id: None,
        }],
    }
}

#[tokio::test]
async fn tool_observation_refs_action() {
    let tmp = TempDir::new("act");
    let store = JsonlStore::open(tmp.path().join("memory.jsonl")).unwrap();
    let model = Fake(Assistant {
        content: "using echo".into(),
        tool_calls: vec![ToolCall {
            id: "c1".into(),
            name: "echo".into(),
            arguments: r#"{"text":"z"}"#.into(),
        }],
    });
    let tools: Vec<Arc<dyn Tool>> = vec![Arc::new(Echo)];
    let actx = ActCtx {
        store: Some(&store),
        agent_id: "agent-1",
        session: "session-1",
        card: Some("card-hash-1"),
    };
    let _ = step(one_user_msg("z"), &actx, &model, &tools, &ctx())
        .await
        .unwrap();
    let events = store.read_all().unwrap();
    let action = events.iter().find(|e| e.kind == Kind::Action).unwrap();
    let obs = events.iter().find(|e| e.kind == Kind::Observation).unwrap();
    assert_eq!(obs.refs, vec![action.id.clone()]);
    assert_eq!(obs.content, "z");
    assert_eq!(action.act.as_ref().unwrap().tool.as_deref(), Some("echo"));
    assert!(obs.act.as_ref().unwrap().effect.is_some());
    assert_eq!(action.card.as_deref(), Some("card-hash-1"));
    assert_eq!(obs.card.as_deref(), Some("card-hash-1"));
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
    let actx = ActCtx {
        store: None,
        agent_id: "a",
        session: "s",
        card: None,
    };
    let next = step(one_user_msg("x"), &actx, &model, &tools, &ctx())
        .await
        .unwrap();
    assert_eq!(next.messages.last().unwrap().content.as_deref(), Some("forbidden"));
}
