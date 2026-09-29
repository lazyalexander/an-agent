//! silk link probe, P0-1: envelopes and delivery. A policy spool yields a
//! silk tell as its continuation; the driver stamps and tapes it; a second
//! policy answers an ask by correlation. Asserts on the tape only:
//! stamping, refs interlock, single termination, and that a script naming
//! its own sender is refused.

// Helper fns here are not #[test] fns, so allow-*-in-tests does not reach them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[allow(dead_code)]
mod support;

use std::sync::Arc;

use an_agent_core::act::{ActCtx, FileFacet, MemoryFacet, Tool, ToolCtx};
use an_agent_core::memstream::{JsonlStore, Kind};
use an_agent_spool::descriptor::{Net, Proc};
use an_agent_spool::spool::{Faces, Flow, Registry};
use serde_json::{Map, json};
use support::mount::{HostCtors, Mounter};
use support::silk::{Envelope, OutEnvelope, SilkKind};
use support::{AgentError, Assistant, Model, TempDir, policy_step};

const SESSION: &str = "s1";

/// A policy that tells bob "hello" once, then halts.
const HERALD_YAML: &str = r#"
v: 1
kind: spool
name: herald
version: 1.0.0
summary: tell bob hello, then halt
constructor: rhai
script: |
  let told = params.clips.filter(|c| c.who == config.name && c.kind == "action" && c.tags.contains("silk"));
  if told.is_empty() {
      #{ kind: "silk", silk: #{ kind: "tell", to: "bob", payload: #{ text: "hello" } } }
  } else {
      #{ kind: "halt" }
  }
effect:
  file: { op: none }
  memory: { op: ignore }
  net: none
  proc: none
  flow: out
inverse: none
requires:
  - { name: mailbox, version: 1.0.0 }
"#;

/// A self-naming policy: malformed by S11, refused, attempt taped.
const FORGER_YAML: &str = r#"
v: 1
kind: spool
name: forger
version: 1.0.0
summary: tries to name its own sender
constructor: rhai
script: |
  #{ kind: "silk", silk: #{ kind: "tell", from: "bob", to: "bob", payload: #{} } }
effect:
  file: { op: none }
  memory: { op: ignore }
  net: none
  proc: none
  flow: out
inverse: none
requires:
  - { name: mailbox, version: 1.0.0 }
"#;

/// The addressable placeholder both policies name in requires. Pure
/// presence: no behavior, no faces — an entry in the address book.
const MAILBOX_YAML: &str = r#"
v: 1
kind: spool
name: mailbox
version: 1.0.0
summary: addressable sink
constructor: rhai
script: |
  #{ kind: "halt" }
effect:
  file: { op: none }
  memory: { op: ignore }
  net: none
  proc: none
  flow: in
inverse: none
requires: []
"#;

struct NoModel;
impl Model for NoModel {
    async fn complete(
        &self,
        _messages: &[support::ChatMessage],
        _tools: &[Arc<dyn Tool>],
    ) -> Result<Assistant, AgentError> {
        Ok(Assistant {
            content: String::new(),
            tool_calls: vec![],
            usage: None,
        })
    }
}

fn ctx() -> ToolCtx {
    let (_tx, rx) = tokio::sync::watch::channel(false);
    ToolCtx { signal: Some(rx) }
}

fn rights() -> Faces {
    Faces {
        file: FileFacet::None,
        memory: MemoryFacet::Ignore,
        net: Net::None,
        proc_: Proc::None,
        flow: Flow::Both,
    }
}

fn silk_envelopes(store: &JsonlStore) -> Vec<Envelope> {
    store
        .read_all()
        .unwrap()
        .iter()
        .filter(|e| e.tags.iter().any(|t| t == "silk"))
        .filter_map(|e| serde_json::from_str(&e.content).ok())
        .collect()
}

#[tokio::test]
async fn policy_tell_is_stamped_and_taped() {
    let tmp = TempDir::new("silk-link");
    let store = JsonlStore::open(tmp.path().join("memory.jsonl")).unwrap();
    let registry = Registry::open(tmp.path().join("spools")).unwrap();
    let hostx = ActCtx {
        store: Some(&store),
        agent_id: "parley-host",
        session: SESSION,
        card: None,
    };
    let mut mounter = Mounter::new(&hostx, &registry, HostCtors::new());
    mounter
        .mount(None, MAILBOX_YAML, Map::new(), &rights())
        .unwrap();
    let herald = mounter
        .mount(
            None,
            HERALD_YAML,
            json!({"name": "ada"}).as_object().unwrap().clone(),
            &rights(),
        )
        .unwrap();

    let actx = ActCtx {
        store: Some(&store),
        agent_id: "ada",
        session: SESSION,
        card: None,
    };
    let policy = mounter.policy(herald).unwrap();
    // Step one yields the tell, step two halts.
    assert!(
        !policy_step(&actx, &NoModel, &policy, &[], &ctx(), None)
            .await
            .unwrap()
    );
    assert!(
        policy_step(&actx, &NoModel, &policy, &[], &ctx(), None)
            .await
            .unwrap()
    );

    let envelopes = silk_envelopes(&store);
    assert_eq!(envelopes.len(), 1);
    let env = &envelopes[0];
    assert_eq!(env.kind, SilkKind::Tell);
    assert_eq!(env.from, "ada"); // stamped with the mount's tape identity
    assert_eq!(env.to, "bob");
    assert_eq!(env.payload, json!({"text": "hello"}));
    assert_eq!(env.v, 0);
}

#[tokio::test]
async fn self_named_sender_is_refused_and_the_attempt_is_taped() {
    let tmp = TempDir::new("silk-forge");
    let store = JsonlStore::open(tmp.path().join("memory.jsonl")).unwrap();
    let registry = Registry::open(tmp.path().join("spools")).unwrap();
    let hostx = ActCtx {
        store: Some(&store),
        agent_id: "parley-host",
        session: SESSION,
        card: None,
    };
    let mut mounter = Mounter::new(&hostx, &registry, HostCtors::new());
    mounter
        .mount(None, MAILBOX_YAML, Map::new(), &rights())
        .unwrap();
    let forger = mounter
        .mount(None, FORGER_YAML, Map::new(), &rights())
        .unwrap();

    let actx = ActCtx {
        store: Some(&store),
        agent_id: "eve",
        session: SESSION,
        card: None,
    };
    let policy = mounter.policy(forger).unwrap();
    // The malformed continuation fails at the policy's own boundary: the
    // step errors, and the reason is on tape as the decision's effect.
    let err = policy_step(&actx, &NoModel, &policy, &[], &ctx(), None)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("not a continuation"), "{err}");
    // No envelope landed; the refusal reason is on tape.
    assert!(silk_envelopes(&store).is_empty());
    let events = store.read_all().unwrap();
    assert!(events.iter().any(|e| {
        e.kind == Kind::Observation
            && e.act
                .as_ref()
                .is_some_and(|a| a.tool.as_deref() == Some("forger"))
            && e.content.contains("driver stamps")
    }));
}

#[tokio::test]
async fn ask_reply_terminates_exactly_once() {
    let tmp = TempDir::new("silk-ask");
    let store = JsonlStore::open(tmp.path().join("memory.jsonl")).unwrap();

    let ask_id = support::silk::deliver(
        &store,
        "ada",
        SESSION,
        OutEnvelope {
            kind: SilkKind::Ask,
            to: "bob".into(),
            call_id: None,
            wake: true,
            payload: json!({"q": "ping"}),
        },
    )
    .unwrap();
    let call_id = silk_envelopes(&store)[0].call_id.clone().unwrap();

    // The reply interlocks by refs; every later terminal is refused.
    let reply_id = support::silk::deliver(
        &store,
        "bob",
        SESSION,
        OutEnvelope {
            kind: SilkKind::Reply,
            to: "ada".into(),
            call_id: Some(call_id.clone()),
            wake: false,
            payload: json!({"a": "pong"}),
        },
    )
    .unwrap();
    let events = store.read_all().unwrap();
    let reply_event = events.iter().find(|e| e.id == reply_id).unwrap();
    assert_eq!(reply_event.refs, vec![ask_id]);
    assert!(
        support::silk::cancel_ask(&store, "ada", SESSION, &call_id, "late")
            .unwrap_err()
            .to_string()
            .contains("terminated")
    );
}
