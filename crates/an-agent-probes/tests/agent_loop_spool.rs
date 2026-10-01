//! The agent loop as a spool: `fixtures/agent_loop.yaml` is the whole
//! loop body. The model proposes tool calls (bounded to the requires
//! whitelist); the policy approves them by reference (clip + index, args
//! never enter the script); every hop is on tape. Also asserted: approving
//! a call outside the requires closure is refused and executes nothing.

// Helper fns here are not #[test] fns, so allow-*-in-tests does not reach them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[allow(dead_code)]
mod support;

use std::sync::{Arc, Mutex};

use an_agent_core::act::{ActCtx, Tool, ToolCall, ToolCtx, ToolTag};
use an_agent_core::memstream::{AppendEvent, FromKind, JsonlStore, Kind};
use an_agent_spool::spool::Registry;
use serde_json::{Map, Value, json};
use support::mount::{HostCtors, Mounter};
use support::{
    AgentError, Assistant, ChatMessage, Model, StepOpts, TempDir, Usage, run_policy_until_idle,
};

const SESSION: &str = "s1";

/// The library is the source of the bodies — the probe is also the
/// library's CI: a malformed entry fails here first.
fn library_body(name: &str) -> String {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../spools")
        .join(name)
        .join(format!("{name}.yaml"));
    an_agent_spool::library::load(&path).unwrap()
}

/// A model with a canned script of responses.
struct ScriptedModel {
    replies: Mutex<std::collections::VecDeque<Assistant>>,
    tools_seen: Arc<Mutex<Vec<Vec<String>>>>,
}

impl Model for ScriptedModel {
    async fn complete(
        &self,
        _messages: &[ChatMessage],
        tools: &[Arc<dyn Tool>],
    ) -> Result<Assistant, AgentError> {
        self.tools_seen
            .lock()
            .unwrap()
            .push(tools.iter().map(|t| t.name().to_string()).collect());
        Ok(self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .expect("scripted model ran out of replies"))
    }
}

struct Echo {
    calls: Mutex<Vec<String>>,
}

#[async_trait::async_trait]
impl Tool for Echo {
    fn name(&self) -> &str {
        "echo"
    }

    fn description(&self) -> &str {
        "echo"
    }

    fn parameters(&self) -> Value {
        json!({})
    }

    fn tag_seed(&self) -> Option<ToolTag> {
        Some(ToolTag::none())
    }

    async fn execute(&self, args: Value, _ctx: &ToolCtx) -> Result<String, String> {
        let text = args["text"].as_str().unwrap_or("").to_string();
        self.calls.lock().unwrap().push(text.clone());
        Ok(text)
    }
}

fn ctx() -> ToolCtx {
    let (_tx, rx) = tokio::sync::watch::channel(false);
    ToolCtx { signal: Some(rx) }
}

fn rights() -> an_agent_spool::spool::Faces {
    use an_agent_core::act::{FileFacet, MemoryFacet};
    use an_agent_spool::descriptor::{Net, Proc};
    use an_agent_spool::spool::Flow;
    an_agent_spool::spool::Faces {
        file: FileFacet::None,
        memory: MemoryFacet::Ignore,
        net: Net::None,
        proc_: Proc::None,
        flow: Flow::Both,
    }
}

fn seed_task(store: &JsonlStore) {
    store
        .append(AppendEvent {
            from: "user".into(),
            from_kind: FromKind::Human,
            kind: Kind::Utterance,
            session: SESSION.into(),
            content: "ping the echo".into(),
            tags: vec![],
            refs: vec![],
            act: None,
            card: None,
        })
        .unwrap();
}

fn loop_ctx(store: &JsonlStore) -> ActCtx<'_> {
    ActCtx {
        store: Some(store),
        agent_id: "lo",
        session: SESSION,
        card: None,
    }
}

fn invoke_effects(
    events: &[an_agent_core::memstream::Memevent],
) -> Vec<&an_agent_core::memstream::Memevent> {
    events
        .iter()
        .filter(|e| {
            e.kind == Kind::Observation
                && e.act
                    .as_ref()
                    .is_some_and(|a| a.kind == "invoke" && a.tool.is_none() && a.effect.is_some())
        })
        .collect()
}

#[tokio::test]
async fn the_loop_propose_approve_execute_halt() {
    let tmp = TempDir::new("loop-spool");
    let store = JsonlStore::open(tmp.path().join("memory.jsonl")).unwrap();
    let registry = Registry::open(tmp.path().join("spools")).unwrap();

    let echo = Arc::new(Echo {
        calls: Mutex::new(vec![]),
    });
    let mut hosts = HostCtors::new();
    {
        let echo = echo.clone();
        hosts.insert(
            "echo".into(),
            Box::new(move |_cfg: &Map<String, Value>| Ok(echo.clone() as Arc<dyn Tool>)),
        );
    }
    let hostx = ActCtx {
        store: Some(&store),
        agent_id: "parley-host",
        session: SESSION,
        card: None,
    };
    let mut mounter = Mounter::new(&hostx, &registry, hosts);
    let echo_body = library_body("echo");
    let agent_loop_body = library_body("agent_loop");
    mounter
        .mount(None, &echo_body, Map::new(), &rights(), None, None)
        .unwrap();
    let lo = mounter
        .mount(
            None,
            &agent_loop_body,
            json!({"name": "lo"}).as_object().unwrap().clone(),
            &rights(),
            Some("lo".into()),
            None,
        )
        .unwrap();

    let tools_seen = Arc::new(Mutex::new(vec![]));
    let model = ScriptedModel {
        replies: Mutex::new(std::collections::VecDeque::from([
            Assistant {
                content: "calling echo".into(),
                tool_calls: vec![ToolCall {
                    id: "c1".into(),
                    name: "echo".into(),
                    arguments: r#"{"text":"ping"}"#.into(),
                }],
                usage: Some(Usage {
                    prompt_tokens: 5,
                    completion_tokens: 3,
                }),
            },
            Assistant {
                content: "pong: ping".into(),
                tool_calls: vec![],
                usage: None,
            },
        ])),
        tools_seen: tools_seen.clone(),
    };

    seed_task(&store);
    let echo_tool = mounter.resolve("echo").unwrap();
    let actx = loop_ctx(&store);
    let policy = mounter.policy(lo).unwrap();
    run_policy_until_idle(
        &actx,
        &model,
        &policy,
        std::slice::from_ref(&echo_tool),
        &ctx(),
        8,
        StepOpts::default(),
    )
    .await
    .unwrap();

    let events = store.read_all().unwrap();
    // Two model calls, each its own intent/effect pair; the first effect
    // carries the full proposal, args included.
    let effects = invoke_effects(&events);
    assert_eq!(effects.len(), 2);
    assert!(effects[0].content.contains(r#""name":"echo""#));
    // arguments ride as a wire string, escaped inside the effect JSON.
    assert!(effects[0].content.contains(r#"{\"text\":\"ping\"}"#));
    // The model only ever saw the whitelisted tool.
    assert_eq!(
        tools_seen.lock().unwrap().as_slice(),
        [vec!["echo".to_string()], vec!["echo".to_string()]]
    );
    // The policy's approve decision is on tape.
    assert!(events.iter().any(|e| {
        e.kind == Kind::Observation
            && e.from == "lo"
            && e.act
                .as_ref()
                .is_some_and(|a| a.tool.as_deref() == Some("agent_loop"))
            && e.content.contains("\"approve\"")
    }));
    // Echo ran exactly once, through admission, with the proposed args.
    assert_eq!(echo.calls.lock().unwrap().as_slice(), ["ping"]);
    let echo_obs = events
        .iter()
        .find(|e| {
            e.kind == Kind::Observation
                && e.act
                    .as_ref()
                    .is_some_and(|a| a.tool.as_deref() == Some("echo"))
        })
        .unwrap();
    assert_eq!(echo_obs.content, "ping");
    // The final answer is uttered by the loop's identity.
    assert!(
        events
            .iter()
            .any(|e| { e.kind == Kind::Utterance && e.from == "lo" && e.content == "pong: ping" })
    );
}

#[tokio::test]
async fn approving_outside_the_requires_closure_executes_nothing() {
    let tmp = TempDir::new("loop-rogue");
    let store = JsonlStore::open(tmp.path().join("memory.jsonl")).unwrap();
    let registry = Registry::open(tmp.path().join("spools")).unwrap();

    let echo = Arc::new(Echo {
        calls: Mutex::new(vec![]),
    });
    let mut hosts = HostCtors::new();
    {
        let echo = echo.clone();
        hosts.insert(
            "echo".into(),
            Box::new(move |_cfg: &Map<String, Value>| Ok(echo.clone() as Arc<dyn Tool>)),
        );
    }
    let hostx = ActCtx {
        store: Some(&store),
        agent_id: "parley-host",
        session: SESSION,
        card: None,
    };
    let mut mounter = Mounter::new(&hostx, &registry, hosts);
    let echo_body = library_body("echo");
    let agent_loop_body = library_body("agent_loop");
    mounter
        .mount(None, &echo_body, Map::new(), &rights(), None, None)
        .unwrap();
    let lo = mounter
        .mount(
            None,
            &agent_loop_body,
            json!({"name": "lo"}).as_object().unwrap().clone(),
            &rights(),
            Some("lo".into()),
            None,
        )
        .unwrap();

    let model = ScriptedModel {
        replies: Mutex::new(std::collections::VecDeque::from([Assistant {
            content: String::new(),
            tool_calls: vec![ToolCall {
                id: "c1".into(),
                name: "bash".into(),
                arguments: r#"{"command":"rm -rf /"}"#.into(),
            }],
            usage: None,
        }])),
        tools_seen: Arc::new(Mutex::new(vec![])),
    };

    seed_task(&store);
    let echo_tool = mounter.resolve("echo").unwrap();
    let actx = loop_ctx(&store);
    let policy = mounter.policy(lo).unwrap();
    let err = run_policy_until_idle(
        &actx,
        &model,
        &policy,
        std::slice::from_ref(&echo_tool),
        &ctx(),
        8,
        StepOpts::default(),
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("outside its requires"), "{err}");
    // Nothing executed; the proposal and the refused approval are on tape.
    assert!(echo.calls.lock().unwrap().is_empty());
    let events = store.read_all().unwrap();
    let effects = invoke_effects(&events);
    assert_eq!(effects.len(), 1);
    assert!(effects[0].content.contains("bash"));
    assert!(events.iter().any(|e| {
        e.kind == Kind::Observation
            && e.act
                .as_ref()
                .is_some_and(|a| a.tool.as_deref() == Some("agent_loop"))
            && e.content.contains("\"approve\"")
    }));
}
