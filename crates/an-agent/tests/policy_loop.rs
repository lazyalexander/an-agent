//! Policy-driven loop probe: control flow lives in a rhai script mounted
//! on tape (`fixtures/echo_policy.yaml`). The script sees a tape
//! projection and yields one continuation per step; the host admits each
//! decision through `run_tool_act` and executes it. Asserts the tape
//! alone shows: the mount (script + hash), per-step policy decisions,
//! the model call as its own invoke intent/effect pair, and the
//! whitelisted tool call.

#[allow(dead_code)]
mod support;

use std::sync::Arc;

use an_agent::act::{ActCtx, Tool, ToolCtx, ToolTag};
use an_agent::memstream::{AppendEvent, FromKind, JsonlStore, Kind};
use serde_json::json;
use support::rhai::RhaiPolicy;
use support::{
    AgentError, Assistant, ChatMessage, Model, TempDir, Usage, mount_policy, policy_step,
    run_policy_until_idle,
};

const POLICY_YAML: &str = include_str!("fixtures/echo_policy.yaml");

struct Fake;
impl Model for Fake {
    async fn complete(
        &self,
        _messages: &[ChatMessage],
        _tools: &[Arc<dyn Tool>],
    ) -> Result<Assistant, AgentError> {
        Ok(Assistant {
            content: "model says hi".into(),
            tool_calls: vec![],
            usage: Some(Usage {
                prompt_tokens: 3,
                completion_tokens: 2,
            }),
        })
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

fn ctx() -> ToolCtx {
    let (_tx, rx) = tokio::sync::watch::channel(false);
    ToolCtx { signal: Some(rx) }
}

fn seed_task(store: &JsonlStore, session: &str) -> Result<(), an_agent::memstream::StoreError> {
    store.append(AppendEvent {
        from: "user".into(),
        from_kind: FromKind::Human,
        kind: Kind::Utterance,
        session: session.into(),
        content: "say hi".into(),
        tags: vec![],
        refs: vec![],
        act: None,
        card: None,
    })?;
    Ok(())
}

#[tokio::test]
async fn policy_drives_model_tool_and_halt() {
    let tmp = TempDir::new("policy");
    let store = JsonlStore::open(tmp.path().join("memory.jsonl")).unwrap();
    let actx = ActCtx {
        store: Some(&store),
        agent_id: "agent-1",
        session: "s1",
        card: Some("card-hash"),
    };
    let desc = an_agent::tools::descriptor::parse(POLICY_YAML).unwrap();
    let policy = Arc::new(RhaiPolicy::from_descriptor(desc).unwrap());
    let hash = mount_policy(&actx, policy.name(), POLICY_YAML)
        .unwrap()
        .unwrap();
    seed_task(&store, "s1").unwrap();

    let tools: Vec<Arc<dyn Tool>> = vec![Arc::new(Echo)];
    run_policy_until_idle(&actx, &Fake, &policy, &tools, &ctx(), 10)
        .await
        .unwrap();

    let events = store.read_all().unwrap();
    // The mount event carries the full descriptor (script inline) and its
    // content hash — the audit/reuse source of the run's control flow.
    let mount = events
        .iter()
        .find(|e| e.tags.contains(&"policy".to_string()))
        .unwrap();
    assert!(mount.content.contains("constructor: rhai"));
    assert!(mount.tags.contains(&hash));
    assert_eq!(mount.act.as_ref().unwrap().kind, "mount");
    // The model call kept its own invoke intent/effect pair — a replay
    // anchor distinct from the policy decisions around it. Count by act
    // metadata: later policy intents embed tape previews in their args,
    // so content-substring matching would count echoes of the hash.
    let invoke_intents = events
        .iter()
        .filter(|e| {
            e.kind == Kind::Action
                && e.act
                    .as_ref()
                    .is_some_and(|a| a.kind == "invoke" && a.tool.is_none())
        })
        .count();
    assert_eq!(invoke_intents, 1);
    let invoke_effects = events
        .iter()
        .filter(|e| {
            e.kind == Kind::Observation
                && e.act
                    .as_ref()
                    .is_some_and(|a| a.kind == "invoke" && a.tool.is_none() && a.effect.is_some())
        })
        .count();
    assert_eq!(invoke_effects, 1);
    // The whitelisted tool ran through admission; its result is on tape.
    assert!(
        events
            .iter()
            .any(|e| e.kind == Kind::Observation && e.content == "model says hi")
    );
    // Every policy step taped its decision, addressed by act metadata
    // (serde_json orders map keys alphabetically, so content-prefix
    // matching on "kind" would miss most continuations).
    let decisions = events
        .iter()
        .filter(|e| {
            e.kind == Kind::Observation
                && e.act
                    .as_ref()
                    .is_some_and(|a| a.tool.as_deref() == Some("echo_policy"))
        })
        .count();
    assert_eq!(decisions, 4);
    // The policy's own utterance and the halt both landed.
    assert!(
        events
            .iter()
            .any(|e| e.kind == Kind::Utterance && e.content == "done")
    );
    assert!(
        events
            .iter()
            .any(|e| e.kind == Kind::Observation && e.content == "{\"kind\":\"halt\"}")
    );
}

#[tokio::test]
async fn policy_cannot_yield_tool_outside_requires() {
    let yaml = "v: 1\nname: rogue\nversion: 0.1.0\nconstructor: rhai\nscript: |\n  #{ kind: \"invoke_tool\", name: \"echo\", args: #{ text: \"x\" } }\nsummary: rogue policy\neffect:\n  file: { op: none }\n  net: none\n  proc: none\n  memory: { op: ignore }\nrequires: []\n";
    let tmp = TempDir::new("policy-rogue");
    let store = JsonlStore::open(tmp.path().join("memory.jsonl")).unwrap();
    let actx = ActCtx {
        store: Some(&store),
        agent_id: "agent-1",
        session: "s1",
        card: None,
    };
    let desc = an_agent::tools::descriptor::parse(yaml).unwrap();
    let policy = Arc::new(RhaiPolicy::from_descriptor(desc).unwrap());
    let tools: Vec<Arc<dyn Tool>> = vec![Arc::new(Echo)];

    let err = policy_step(&actx, &Fake, &policy, &tools, &ctx())
        .await
        .unwrap_err();
    assert!(matches!(err, AgentError::Model(_)));
    assert!(err.to_string().contains("requires"));
    // The decision is on tape; the call never happened.
    let events = store.read_all().unwrap();
    assert!(
        events
            .iter()
            .any(|e| e.kind == Kind::Observation && e.content.contains("\"invoke_tool\""))
    );
    assert!(
        !events
            .iter()
            .any(|e| e.kind == Kind::Observation && e.content == "x")
    );
}
