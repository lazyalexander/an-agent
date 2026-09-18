//! Loop behavior tests for the probe ReAct loop (tests/support): utterance
//! admission, observation→action refs, and that a Forbidden permit never
//! reaches execution. Migrated from the former src/agent.rs unit tests.

#[allow(dead_code)]
mod support;

use std::sync::Arc;

use an_agent::act::{ActCtx, Permit, Tool, ToolCall, ToolCtx, ToolTag, run_tool_act};
use an_agent::memstream::{JsonlStore, Kind, Memevent};
use serde_json::json;
use support::{AgentError, AgentState, Assistant, ChatMessage, Model, TempDir, step};

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

struct Rem;
#[async_trait::async_trait]
impl Tool for Rem {
    fn name(&self) -> &str {
        "remember"
    }
    fn description(&self) -> &str {
        "remember"
    }
    fn parameters(&self) -> serde_json::Value {
        json!({})
    }
    fn tag_seed(&self) -> Option<ToolTag> {
        Some(ToolTag::remember())
    }
    async fn execute(&self, args: serde_json::Value, _ctx: &ToolCtx) -> Result<String, String> {
        Ok(args
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string())
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
        usage: None,
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
    let is_tool = |e: &Memevent| e.act.as_ref().and_then(|a| a.tool.as_deref()) == Some("echo");
    let action = events
        .iter()
        .find(|e| e.kind == Kind::Action && is_tool(e))
        .unwrap();
    let obs = events
        .iter()
        .find(|e| e.kind == Kind::Observation && e.refs.first() == Some(&action.id))
        .unwrap();
    assert_eq!(obs.refs, vec![action.id.clone()]);
    assert_eq!(obs.content, "z");
    assert!(obs.act.as_ref().unwrap().effect.is_some());
    assert_eq!(action.card.as_deref(), Some("card-hash-1"));
    assert_eq!(obs.card.as_deref(), Some("card-hash-1"));

    // The model call itself is taped as an invoke act: intent has no tool
    // (channel = model side), its observation refs back to it. Tool acts
    // share the invoke domain, so filter on the absent tool field.
    let invoke = events
        .iter()
        .find(|e| {
            e.kind == Kind::Action
                && e.act.as_ref().map(|a| a.kind.as_str()) == Some("invoke")
                && e.act.as_ref().and_then(|a| a.tool.as_deref()).is_none()
        })
        .unwrap();
    let invoke_obs = events
        .iter()
        .find(|e| e.kind == Kind::Observation && e.refs.first() == Some(&invoke.id))
        .unwrap();
    assert!(invoke_obs.act.as_ref().unwrap().effect.is_some());
}

#[tokio::test]
async fn remember_clip_is_written_by_admission_not_the_tool() {
    let tmp = TempDir::new("act");
    let store = JsonlStore::open(tmp.path().join("memory.jsonl")).unwrap();
    let actx = ActCtx {
        store: Some(&store),
        agent_id: "agent-1",
        session: "session-1",
        card: None,
    };
    let tools: Vec<Arc<dyn Tool>> = vec![Arc::new(Echo), Arc::new(Rem)];

    // An evidence-producing call first, so the clip has something to cite.
    run_tool_act(
        &actx,
        &tools,
        &ToolCall {
            id: "c1".into(),
            name: "echo".into(),
            arguments: r#"{"text":"evidence"}"#.into(),
        },
        &ctx(),
    )
    .await
    .unwrap();
    let r = run_tool_act(
        &actx,
        &tools,
        &ToolCall {
            id: "c2".into(),
            name: "remember".into(),
            arguments: r#"{"text":"sky is blue"}"#.into(),
        },
        &ctx(),
    )
    .await
    .unwrap();
    assert!(r.message.content.starts_with("remembered as"));

    let events = store.read_all().unwrap();
    let clip = events
        .iter()
        .find(|e| e.tags.iter().any(|t| t == "memory"))
        .expect("no memory clip");
    assert_eq!(clip.kind, Kind::Utterance);
    assert_eq!(clip.content, "sky is blue");
    assert_eq!(clip.act.as_ref().unwrap().kind.as_str(), "remember");
    // Kinship: the clip refs the latest observation (the echo evidence),
    // not its own call's observation.
    let evidence = events
        .iter()
        .find(|e| e.kind == Kind::Observation && e.content == "evidence")
        .unwrap();
    assert_eq!(clip.refs, vec![evidence.id.clone()]);
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
        usage: None,
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
    assert_eq!(
        next.messages.last().unwrap().content.as_deref(),
        Some("forbidden")
    );
}
