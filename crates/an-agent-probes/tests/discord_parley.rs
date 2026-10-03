//! Discord parley probe: two spools — a channel bridge (`discord`) and a
//! character policy (`ai_character`) — mounted into one session give a
//! Discord channel multiple AI characters. The bridge here is a scripted
//! probe tool standing in for the real transport; the tape alone must
//! replay the whole parley: mounts, ingress utterances, per-character
//! model calls, sends, unmounts.

// Helper fns here are not #[test] fns, so allow-*-in-tests does not reach them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[allow(dead_code)]
mod support;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use an_agent_core::act::{
    ActCtx, FileFacet, MemoryFacet, Permit, Tool, ToolCall, ToolCtx, ToolTag, run_tool_act,
};
use an_agent_core::memstream::{AppendEvent, FromKind, JsonlStore, Kind};
use an_agent_spool::descriptor::{Net, Proc};
use an_agent_spool::spool::{Faces, Flow, Registry};
use serde_json::{Map, Value, json};
use support::mount::{HostCtors, Mounter};
use support::{AgentError, Assistant, Model, StepOpts, TempDir, run_policy_until_idle};

const DISCORD_YAML: &str = include_str!("fixtures/discord.yaml");
const CHARACTER_YAML: &str = include_str!("fixtures/ai_character.yaml");

const SESSION: &str = "s1";

// --- scripted channel bridge (probe-grade stand-in for the real spool body) ---

/// The two behaviors the parley depends on, held to the same discipline
/// the real bridge must keep: bot-authored messages (including our own
/// sends) are dropped before they can reach the tape, so agent loops and
/// character recursion are impossible by construction; and the poll
/// cursor only advances past a fully parsed batch (snowflakes compare as
/// numbers, not strings).
struct ScriptedBridge {
    poll_bodies: Mutex<VecDeque<String>>,
    posts: Mutex<Vec<String>>,
    after: Mutex<Option<u64>>,
}

impl ScriptedBridge {
    fn new(bodies: Vec<String>) -> Self {
        Self {
            poll_bodies: Mutex::new(bodies.into()),
            posts: Mutex::new(vec![]),
            after: Mutex::new(None),
        }
    }

    fn poll(&self) -> Result<String, String> {
        let body = self
            .poll_bodies
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| "[]".into());
        let raw: Vec<Value> =
            serde_json::from_str(&body).map_err(|e| format!("poll body is not a list: {e}"))?;
        let mut cursor = *self.after.lock().unwrap();
        let mut out = Vec::new();
        for m in &raw {
            let id: u64 = m["id"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .ok_or("message id is not a snowflake")?;
            if !m["author"]["bot"].as_bool().unwrap_or(false) {
                out.push(json!({
                    "id": m["id"].as_str().unwrap_or_default(),
                    "author": m["author"]["username"].as_str().unwrap_or("unknown"),
                    "content": m["content"].as_str().unwrap_or(""),
                }));
            }
            cursor = Some(cursor.map_or(id, |c: u64| c.max(id)));
        }
        *self.after.lock().unwrap() = cursor;
        Ok(Value::Array(out).to_string())
    }

    fn send(&self, content: &str) -> Result<String, String> {
        let mut posts = self.posts.lock().unwrap();
        // Discord's message cap, enforced at the bridge.
        let content: String = content.chars().take(2000).collect();
        posts.push(content);
        Ok(format!("9{:02}", posts.len()))
    }
}

#[async_trait::async_trait]
impl Tool for ScriptedBridge {
    fn name(&self) -> &str {
        "discord"
    }

    fn description(&self) -> &str {
        "scripted discord channel"
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "op": { "type": "string", "enum": ["poll", "send"] },
                "content": { "type": "string" }
            },
            "required": ["op"]
        })
    }

    fn tag_seed(&self) -> Option<ToolTag> {
        // net/flow are not in ToolTag's three faces — the egress face is
        // carried by the spool declaration and its closure fold, not by
        // the admission tag. This asymmetry is the current state.
        Some(ToolTag::none())
    }

    async fn execute(&self, args: Value, _ctx: &ToolCtx) -> Result<String, String> {
        match args["op"].as_str() {
            Some("poll") => self.poll(),
            Some("send") => self.send(args["content"].as_str().ok_or("send requires content")?),
            other => Err(format!("unknown op: {other:?}")),
        }
    }
}

// --- scripted model ---

struct FixedModel {
    reply: String,
    system_seen: Arc<Mutex<Vec<String>>>,
}

impl Model for FixedModel {
    async fn complete(
        &self,
        messages: &[support::ChatMessage],
        _tools: &[Arc<dyn Tool>],
    ) -> Result<Assistant, AgentError> {
        if let Some(first) = messages.first()
            && first.role == "system"
        {
            self.system_seen
                .lock()
                .unwrap()
                .push(first.content.clone().unwrap_or_default());
        }
        Ok(Assistant {
            content: self.reply.clone(),
            tool_calls: vec![],
            usage: None,
        })
    }
}

fn ctx() -> ToolCtx {
    let (_tx, rx) = tokio::sync::watch::channel(false);
    ToolCtx { signal: Some(rx) }
}

/// The workspace grants exactly what the parley needs: egress and a
/// bidirectional flow, no file, no proc.
fn rights() -> Faces {
    Faces {
        file: FileFacet::None,
        memory: MemoryFacet::Ignore,
        net: Net::Egress,
        proc_: Proc::None,
        flow: Flow::Both,
    }
}

fn host_ctors(bridge: Arc<ScriptedBridge>) -> HostCtors {
    let mut hosts = HostCtors::new();
    hosts.insert(
        "discord".into(),
        Box::new(move |_cfg: &Map<String, Value>| Ok(bridge.clone() as Arc<dyn Tool>)),
    );
    hosts
}

/// Poll the bridge and land what it returns as human utterances. Ingress
/// is data, not a command: a channel message enters the tape as a plain
/// utterance; neither the bridge nor the driver executes what it carries.
async fn poll_once(
    actx: &ActCtx<'_>,
    bridge: &Arc<dyn Tool>,
    call_id: &mut u64,
) -> Result<(), AgentError> {
    *call_id += 1;
    let call = ToolCall {
        id: format!("poll-{call_id}"),
        name: "discord".into(),
        arguments: json!({"op": "poll"}).to_string(),
    };
    let result = run_tool_act(actx, std::slice::from_ref(bridge), &call, &ctx()).await?;
    let messages: Vec<Value> = serde_json::from_str(&result.message.content)
        .map_err(|e| AgentError::Model(format!("poll output is not a message list: {e}")))?;
    let Some(store) = actx.store else {
        return Ok(());
    };
    for m in messages {
        store.append(AppendEvent {
            from: m["author"].as_str().unwrap_or("unknown").into(),
            from_kind: FromKind::Human,
            kind: Kind::Utterance,
            session: actx.session.into(),
            content: m["content"].as_str().unwrap_or("").into(),
            tags: vec!["discord".into()],
            refs: vec![],
            act: None,
            card: None,
        })?;
    }
    Ok(())
}

fn character_config(name: &str) -> Map<String, Value> {
    json!({ "name": name })
        .as_object()
        .expect("character config is an object")
        .clone()
}

#[tokio::test]
async fn two_characters_answer_a_mention() {
    let tmp = TempDir::new("parley");
    let store = JsonlStore::open(tmp.path().join("memory.jsonl")).unwrap();
    let registry = Registry::open(tmp.path().join("spools")).unwrap();

    // First poll: a human mention of both characters, plus bot-authored
    // noise that must never reach the tape. Second poll: the characters'
    // own sends coming back from the channel — also bot-authored.
    let scripted = Arc::new(ScriptedBridge::new(vec![
        json!([
            {"id": "100", "content": "robot noise", "author": {"id": "9", "username": "other-bot", "bot": true}},
            {"id": "101", "content": "hi @ada and @bob, what do you think of rust?", "author": {"id": "1", "username": "alex"}}
        ])
        .to_string(),
        json!([
            {"id": "202", "content": "bob says: borrow checker", "author": {"id": "8", "username": "parley-bot", "bot": true}},
            {"id": "201", "content": "ada says: fearless concurrency", "author": {"id": "8", "username": "parley-bot", "bot": true}}
        ])
        .to_string(),
    ]));
    let hostx = ActCtx {
        store: Some(&store),
        agent_id: "parley-host",
        session: SESSION,
        card: None,
    };
    let mut mounter = Mounter::new(&hostx, &registry, host_ctors(scripted.clone()));
    let bridge = mounter
        .mount(
            None,
            DISCORD_YAML,
            Map::from_iter([("channel_id".into(), Value::String("chan-1".into()))]),
            &rights(),
            None,
            None,
        )
        .unwrap();
    let ada = mounter
        .mount(
            None,
            CHARACTER_YAML,
            character_config("ada"),
            &rights(),
            Some("ada".into()),
            None,
        )
        .unwrap();
    let bob = mounter
        .mount(
            None,
            CHARACTER_YAML,
            character_config("bob"),
            &rights(),
            Some("bob".into()),
            None,
        )
        .unwrap();

    let mut call_id = 0u64;
    let bridge_tool = mounter.resolve("discord").unwrap();
    poll_once(&hostx, &bridge_tool, &mut call_id).await.unwrap();

    for (name, scope, reply) in [
        ("ada", ada, "ada says: fearless concurrency"),
        ("bob", bob, "bob says: borrow checker"),
    ] {
        let actx = ActCtx {
            store: Some(&store),
            agent_id: name,
            session: SESSION,
            card: None,
        };
        let system_seen = Arc::new(Mutex::new(vec![]));
        let model = FixedModel {
            reply: reply.into(),
            system_seen: system_seen.clone(),
        };
        let persona = format!("You are {name}, a character in a discord channel.");
        let policy = mounter.policy(scope).expect("character mounts a policy");
        run_policy_until_idle(
            &actx,
            &model,
            &policy,
            std::slice::from_ref(&bridge_tool),
            &ctx(),
            5,
            StepOpts {
                system: Some(&persona),
                ..StepOpts::default()
            },
        )
        .await
        .unwrap();
        // The persona was injected host-side as a system message.
        assert_eq!(system_seen.lock().unwrap().as_slice(), [persona]);
    }

    // The characters' own sends come back from the channel; the bridge
    // drops them (bot-authored), so the tape never sees them as input.
    poll_once(&hostx, &bridge_tool, &mut call_id).await.unwrap();

    let events = store.read_all().unwrap();
    // Three mounts (bridge + two characters), all admitted.
    let mounts: Vec<_> = events
        .iter()
        .filter(|e| e.kind == Kind::Action && e.act.as_ref().is_some_and(|a| a.kind == "mount"))
        .collect();
    assert_eq!(mounts.len(), 3);
    // The human mention landed once, as a human utterance.
    let ingress: Vec<_> = events
        .iter()
        .filter(|e| e.kind == Kind::Utterance && e.from_kind == FromKind::Human)
        .collect();
    assert_eq!(ingress.len(), 1);
    assert!(ingress[0].content.contains("@ada"));
    assert_eq!(ingress[0].from, "alex");
    // Bot-authored messages — the noise and our own sends — never landed.
    assert!(!events.iter().any(|e| e.content.contains("robot noise")));
    assert!(
        !events
            .iter()
            .any(|e| e.from_kind == FromKind::Human && e.content.contains("fearless concurrency"))
    );
    // Each character made exactly one model call (its own intent/effect
    // pair) and one send.
    for who in ["ada", "bob"] {
        let intents = events
            .iter()
            .filter(|e| {
                e.kind == Kind::Action
                    && e.from == who
                    && e.act
                        .as_ref()
                        .is_some_and(|a| a.kind == "invoke" && a.tool.is_none())
            })
            .count();
        assert_eq!(intents, 1, "{who} should invoke the model once");
    }
    let posts = scripted.posts.lock().unwrap();
    assert_eq!(posts.len(), 2);
    assert!(posts.iter().any(|p| p.contains("fearless concurrency")));
    assert!(posts.iter().any(|p| p.contains("borrow checker")));
    drop(posts);

    // Unmount everything: every mount is matched by an unmount that refs
    // it, and the irreversible bridge says so on tape.
    for scope in [ada, bob, bridge] {
        let report = mounter.unmount(scope).unwrap();
        assert_eq!(report.len(), 1);
    }
    let events = store.read_all().unwrap();
    let mount_ids: Vec<&str> = mounts.iter().map(|e| e.id.as_str()).collect();
    let unmounts: Vec<_> = events
        .iter()
        .filter(|e| e.kind == Kind::Action && e.act.as_ref().is_some_and(|a| a.kind == "unmount"))
        .collect();
    assert_eq!(unmounts.len(), 3);
    for u in &unmounts {
        assert_eq!(u.refs.len(), 1);
        assert!(mount_ids.contains(&u.refs[0].as_str()));
    }
    let bridge_unmount = unmounts
        .iter()
        .find(|e| e.content.contains("\"name\":\"discord\""))
        .unwrap();
    assert!(bridge_unmount.content.contains("irreversible"));
}

#[test]
fn mount_denied_when_rights_do_not_cover_the_closure() {
    let tmp = TempDir::new("parley-deny");
    let store = JsonlStore::open(tmp.path().join("memory.jsonl")).unwrap();
    let registry = Registry::open(tmp.path().join("spools")).unwrap();
    let scripted = Arc::new(ScriptedBridge::new(vec![]));
    let hostx = ActCtx {
        store: Some(&store),
        agent_id: "parley-host",
        session: SESSION,
        card: None,
    };
    let mut mounter = Mounter::new(&hostx, &registry, host_ctors(scripted));
    // No egress in the rights: a net-declaring spool cannot mount.
    let tight = Faces {
        net: Net::None,
        flow: Flow::None,
        ..rights()
    };
    let err = mounter
        .mount(None, DISCORD_YAML, Map::new(), &tight, None, None)
        .unwrap_err();
    assert!(err.to_string().contains("exceeds rights"));
    // The refusal is on tape as a Deny mount intent.
    let events = store.read_all().unwrap();
    let deny = events
        .iter()
        .find(|e| e.kind == Kind::Action && e.act.as_ref().is_some_and(|a| a.kind == "mount"))
        .unwrap();
    assert_eq!(deny.act.as_ref().unwrap().permit, Permit::Deny);
}
