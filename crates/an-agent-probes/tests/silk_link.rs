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
use an_agent_core::memstream::{AppendEvent, FromKind, JsonlStore, Kind};
use an_agent_spool::descriptor::{Net, Proc};
use an_agent_spool::spool::{Faces, Flow, Registry};
use serde_json::{Map, json};
use sha2::Digest;
use support::mount::{HostCtors, Mounter};
use support::silk::{Envelope, OutEnvelope, SilkKind};
use support::{AgentError, Assistant, Model, StepOpts, TempDir, policy_step};

const SESSION: &str = "s1";

/// A policy that tells bob "hello" once, then halts.
const HERALD_YAML: &str = r#"
v: 1
kind: spool
name: herald
version: 1.0.0
summary: tell the mailbox hello, then halt
constructor: rhai
script: |
  let told = params.clips.filter(|c| c.who == config.name && c.kind == "action" && c.tags.contains("tell"));
  if told.is_empty() {
      #{ kind: "silk", silk: #{ kind: "tell", to: "mailbox", payload: #{ text: "hello" } } }
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
  - { name: deaf, version: 1.0.0 }
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
  #{ kind: "silk", silk: #{ kind: "tell", from: "bob", to: "mailbox", payload: #{} } }
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

/// In the address book but deaf: flow none must not receive.
const DEAF_YAML: &str = r#"
v: 1
kind: spool
name: deaf
version: 1.0.0
summary: present but not silk-capable
constructor: rhai
script: |
  #{ kind: "halt" }
effect:
  file: { op: none }
  memory: { op: ignore }
  net: none
  proc: none
  flow: none
inverse: none
requires: []
"#;

/// Sends one tell to a caller-chosen address, then halts — the probe's
/// negative-space instrument (unmounted / outside-closure / deaf targets).
const COURIER_YAML: &str = r#"
v: 1
kind: spool
name: courier
version: 1.0.0
summary: tell the address in config.to, then halt
constructor: rhai
script: |
  let told = params.clips.filter(|c| c.who == config.name && c.kind == "action" && c.tags.contains("tell"));
  if told.is_empty() {
      #{ kind: "silk", silk: #{ kind: "tell", to: config.to, payload: #{} } }
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
  - { name: deaf, version: 1.0.0 }
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
        .mount(None, MAILBOX_YAML, Map::new(), &rights(), None, None)
        .unwrap();
    mounter
        .mount(None, DEAF_YAML, Map::new(), &rights(), None, None)
        .unwrap();
    let herald = mounter
        .mount(
            None,
            HERALD_YAML,
            json!({"name": "ada"}).as_object().unwrap().clone(),
            &rights(),
            Some("ada".into()),
            None,
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
        !policy_step(
            &actx,
            &NoModel,
            &policy,
            &[],
            &ctx(),
            StepOpts {
                silk: Some(&mounter),
                ..StepOpts::default()
            }
        )
        .await
        .unwrap()
    );
    assert!(
        policy_step(
            &actx,
            &NoModel,
            &policy,
            &[],
            &ctx(),
            StepOpts {
                silk: Some(&mounter),
                ..StepOpts::default()
            }
        )
        .await
        .unwrap()
    );

    let envelopes = silk_envelopes(&store);
    assert_eq!(envelopes.len(), 1);
    let env = &envelopes[0];
    assert_eq!(env.kind, SilkKind::Tell);
    assert_eq!(env.from, "ada"); // stamped with the mount's tape identity
    assert_eq!(env.to, "mailbox");
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
        .mount(None, MAILBOX_YAML, Map::new(), &rights(), None, None)
        .unwrap();
    let forger = mounter
        .mount(
            None,
            FORGER_YAML,
            Map::new(),
            &rights(),
            Some("eve".into()),
            None,
        )
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
    let err = policy_step(
        &actx,
        &NoModel,
        &policy,
        &[],
        &ctx(),
        StepOpts {
            silk: Some(&mounter),
            ..StepOpts::default()
        },
    )
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

/// S12's three checks, one policy, three bad targets: unmounted address,
/// mounted but outside the requires closure, in the closure but deaf.
#[tokio::test]
async fn receiver_admission_rejects_and_tapes_the_attempts() {
    let tmp = TempDir::new("silk-gate");
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
        .mount(None, MAILBOX_YAML, Map::new(), &rights(), None, None)
        .unwrap();
    mounter
        .mount(None, DEAF_YAML, Map::new(), &rights(), None, None)
        .unwrap();
    // Mounted, silk-capable, but in nobody's requires closure.
    mounter
        .mount(
            None,
            FORGER_YAML,
            Map::new(),
            &rights(),
            Some("outsider".into()),
            None,
        )
        .unwrap();

    let courier = |to: &str| {
        json!({"name": "cour", "to": to})
            .as_object()
            .unwrap()
            .clone()
    };
    let actx = ActCtx {
        store: Some(&store),
        agent_id: "cour",
        session: SESSION,
        card: None,
    };

    for (to, reason) in [
        ("ghost", "no such silk address"),
        ("outsider", "requires closure"),
        ("deaf", "may not receive"),
    ] {
        let scope = mounter
            .mount(
                None,
                COURIER_YAML,
                courier(to),
                &rights(),
                Some("cour".into()),
                None,
            )
            .unwrap();
        let policy = mounter.policy(scope).unwrap();
        let err = policy_step(
            &actx,
            &NoModel,
            &policy,
            &[],
            &ctx(),
            StepOpts {
                silk: Some(&mounter),
                ..StepOpts::default()
            },
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains(reason), "to {to}: {err}");
        // The attempt is visible on tape as a silk deny with the reason;
        // no envelope landed.
        let events = store.read_all().unwrap();
        assert!(events.iter().any(|e| {
            e.kind == Kind::Action
                && e.tags.iter().any(|t| t == "deny")
                && e.content.contains(reason)
        }));
        assert!(silk_envelopes(&store).is_empty());
        mounter.unmount(scope).unwrap();
    }
}

/// The listener end of the partition test: utter when the inbox shows a
/// silk envelope, halt otherwise.
const LISTENER_YAML: &str = r#"
v: 1
kind: spool
name: listener
version: 1.0.0
summary: utter when a silk envelope is visible
constructor: rhai
script: |
  if params.clips.filter(|c| c.tags.contains("silk")).is_empty() {
      #{ kind: "halt" }
  } else {
      #{ kind: "utter", text: "heard you" }
  }
effect:
  file: { op: none }
  memory: { op: ignore }
  net: none
  proc: none
  flow: in
inverse: none
requires: []
"#;

/// Tells the address in config.to once, then halts; requires the listener.
const LINK_COURIER_YAML: &str = r#"
v: 1
kind: spool
name: link_courier
version: 1.0.0
summary: tell config.to once, then halt
constructor: rhai
script: |
  let told = params.clips.filter(|c| c.who == config.name && c.kind == "action" && c.tags.contains("tell"));
  if told.is_empty() {
      #{ kind: "silk", silk: #{ kind: "tell", to: config.to, payload: #{ q: "ping" } } }
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
  - { name: listener, version: 1.0.0 }
"#;

/// S14 at the projection: the courier lives in s1, the listener in s2.
/// The envelope crosses partitions into the listener's inbox; everything
/// else in each partition is invisible to the other.
#[tokio::test]
async fn inbox_crosses_partitions_and_partitions_hide_the_rest() {
    let tmp = TempDir::new("silk-partition");
    let store = JsonlStore::open(tmp.path().join("memory.jsonl")).unwrap();
    let registry = Registry::open(tmp.path().join("spools")).unwrap();
    let hostx = ActCtx {
        store: Some(&store),
        agent_id: "parley-host",
        session: "host",
        card: None,
    };
    let mut mounter = Mounter::new(&hostx, &registry, HostCtors::new());
    let listener = mounter
        .mount(
            None,
            LISTENER_YAML,
            Map::new(),
            &rights(),
            Some("eve".into()),
            Some("s2".into()),
        )
        .unwrap();
    let courier = mounter
        .mount(
            None,
            LINK_COURIER_YAML,
            json!({"name": "cour", "to": "eve"})
                .as_object()
                .unwrap()
                .clone(),
            &rights(),
            Some("cour".into()),
            Some("s1".into()),
        )
        .unwrap();

    // Partition-local noise that must never cross.
    for (session, text) in [("s1", "MARKER-S1"), ("s2", "MARKER-S2")] {
        store
            .append(AppendEvent {
                from: "someone".into(),
                from_kind: FromKind::Agent,
                kind: Kind::Utterance,
                session: session.into(),
                content: text.into(),
                tags: vec![],
                refs: vec![],
                act: None,
                card: None,
            })
            .unwrap();
    }

    let cour_ctx = ActCtx {
        store: Some(&store),
        agent_id: "cour",
        session: "s1",
        card: None,
    };
    let cour_policy = mounter.policy(courier).unwrap();
    let opts = || StepOpts {
        silk: Some(&mounter),
        ..StepOpts::default()
    };
    // Step one: the tell. Step two: halt — and its projection is what we
    // inspect for the isolation assertion.
    policy_step(&cour_ctx, &NoModel, &cour_policy, &[], &ctx(), opts())
        .await
        .unwrap();
    policy_step(&cour_ctx, &NoModel, &cour_policy, &[], &ctx(), opts())
        .await
        .unwrap();

    let eve_ctx = ActCtx {
        store: Some(&store),
        agent_id: "eve",
        session: "s2",
        card: None,
    };
    let eve_policy = mounter.policy(listener).unwrap();
    // The listener sees the envelope in its inbox and utters.
    assert!(
        !policy_step(&eve_ctx, &NoModel, &eve_policy, &[], &ctx(), opts())
            .await
            .unwrap()
    );

    let events = store.read_all().unwrap();
    // The reply-side tape shows the utterance in s2.
    assert!(
        events
            .iter()
            .any(|e| { e.kind == Kind::Utterance && e.from == "eve" && e.content == "heard you" })
    );
    // The listener's decision projection contains the envelope (its
    // preview carries the payload) but nothing else from s1.
    let eve_decision = events
        .iter()
        .find(|e| {
            e.kind == Kind::Action
                && e.from == "eve"
                && e.act
                    .as_ref()
                    .is_some_and(|a| a.tool.as_deref() == Some("listener"))
        })
        .unwrap();
    assert!(eve_decision.content.contains("ping"));
    assert!(!eve_decision.content.contains("MARKER-S1"));
    // The courier's projections never saw s2's marker either.
    let cour_decisions: Vec<_> = events
        .iter()
        .filter(|e| {
            e.kind == Kind::Action
                && e.from == "cour"
                && e.act
                    .as_ref()
                    .is_some_and(|a| a.tool.as_deref() == Some("link_courier"))
        })
        .collect();
    assert_eq!(cour_decisions.len(), 2);
    assert!(
        cour_decisions
            .iter()
            .all(|e| !e.content.contains("MARKER-S2"))
    );
}

/// The enhancer: sees the envelope, returns a new payload — nothing else
/// is reachable.
const TRANSLATOR_YAML: &str = r#"
v: 1
kind: spool
name: translator
version: 1.0.0
summary: translate the payload text
constructor: rhai
script: |
  let p = params.envelope.payload;
  #{ text: "译:" + p.text }
effect:
  file: { op: none }
  memory: { op: ignore }
  net: none
  proc: none
  flow: both
inverse: none
requires: []
"#;

/// A receiver whose inbox is screened: it delegates filtering to the
/// translator, so the translator sits in its requires closure.
const SCREENED_YAML: &str = r#"
v: 1
kind: spool
name: screened
version: 1.0.0
summary: inbox behind the translator
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
requires:
  - { name: translator, version: 1.0.0 }
  - { name: half, version: 1.0.0 }
"#;

/// In the receiver's closure but one-way: flow out cannot enhance an
/// inbox — the link must see both directions.
const HALF_YAML: &str = r#"
v: 1
kind: spool
name: half
version: 1.0.0
summary: flow out only
constructor: rhai
script: |
  #{ kind: "halt" }
effect:
  file: { op: none }
  memory: { op: ignore }
  net: none
  proc: none
  flow: out
inverse: none
requires: []
"#;

/// Tells the screened box once, then halts.
const TELLER_YAML: &str = r#"
v: 1
kind: spool
name: teller
version: 1.0.0
summary: tell config.to once, then halt
constructor: rhai
script: |
  let told = params.clips.filter(|c| c.who == config.name && c.kind == "action" && c.tags.contains("tell"));
  if told.is_empty() {
      #{ kind: "silk", silk: #{ kind: "tell", to: config.to, payload: #{ text: "hello" } } }
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
  - { name: screened, version: 1.0.0 }
"#;

/// S14's enhance: the translator sits on the screened box's inbox; the
/// envelope lands translated, with both hashes taped and the hop linked
/// into the envelope's refs. Stamped fields are out of the transform's
/// reach by construction.
#[tokio::test]
async fn enhancer_transforms_payload_and_is_audited() {
    let tmp = TempDir::new("silk-enhance");
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
        .mount(
            None,
            TRANSLATOR_YAML,
            Map::new(),
            &rights(),
            Some("tr".into()),
            None,
        )
        .unwrap();
    // screened's body requires half too; publish order is topological.
    mounter
        .mount(
            None,
            HALF_YAML,
            Map::new(),
            &rights(),
            Some("half".into()),
            None,
        )
        .unwrap();
    mounter
        .mount(
            None,
            SCREENED_YAML,
            Map::new(),
            &rights(),
            Some("box".into()),
            None,
        )
        .unwrap();

    // Link-time refusals: unmounted enhancer, enhancer outside the
    // receiver's closure, enhancer that cannot see both directions.
    // The courier's body requires mailbox and deaf; both must be
    // published before it mounts (mailbox goes unused here).
    mounter
        .mount(None, MAILBOX_YAML, Map::new(), &rights(), None, None)
        .unwrap();
    mounter
        .mount(
            None,
            DEAF_YAML,
            Map::new(),
            &rights(),
            Some("deaf".into()),
            None,
        )
        .unwrap();
    let courier_cfg = json!({"name": "cour", "to": "box"})
        .as_object()
        .unwrap()
        .clone();
    mounter
        .mount(
            None,
            COURIER_YAML,
            courier_cfg,
            &rights(),
            Some("cour".into()),
            None,
        )
        .unwrap();
    assert!(
        mounter
            .link_enhancer("box", "ghost")
            .unwrap_err()
            .to_string()
            .contains("not a mounted")
    );
    assert!(
        mounter
            .link_enhancer("box", "cour")
            .unwrap_err()
            .to_string()
            .contains("requires closure")
    );
    assert!(
        mounter
            .link_enhancer("box", "half")
            .unwrap_err()
            .to_string()
            .contains("flow both")
    );

    // The real link, taped as configuration.
    mounter.link_enhancer("box", "tr").unwrap();
    let events = store.read_all().unwrap();
    assert!(events.iter().any(|e| {
        e.tags.iter().any(|t| t == "link") && e.content.contains("\"enhancer\":\"tr\"")
    }));

    let teller = mounter
        .mount(
            None,
            TELLER_YAML,
            json!({"name": "tl", "to": "box"})
                .as_object()
                .unwrap()
                .clone(),
            &rights(),
            Some("tl".into()),
            None,
        )
        .unwrap();
    let tl_ctx = ActCtx {
        store: Some(&store),
        agent_id: "tl",
        session: SESSION,
        card: None,
    };
    let policy = mounter.policy(teller).unwrap();
    policy_step(
        &tl_ctx,
        &NoModel,
        &policy,
        &[],
        &ctx(),
        StepOpts {
            silk: Some(&mounter),
            ..StepOpts::default()
        },
    )
    .await
    .unwrap();

    let events = store.read_all().unwrap();
    let envelopes = silk_envelopes(&store);
    assert_eq!(envelopes.len(), 1);
    let env = &envelopes[0];
    // The payload was transformed; the stamped fields were not.
    assert_eq!(env.payload, json!({"text": "译:hello"}));
    assert_eq!(env.from, "tl");
    assert_eq!(env.to, "box");
    // The enhance hop is taped with both hashes and linked from the
    // envelope's refs.
    let enhance = events
        .iter()
        .find(|e| e.tags.iter().any(|t| t == "enhance"))
        .unwrap();
    let orig_hash = format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(&json!({"text": "hello"})).unwrap())
    );
    let new_hash = format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(&json!({"text": "译:hello"})).unwrap())
    );
    assert!(enhance.content.contains(&orig_hash));
    assert!(enhance.content.contains(&new_hash));
    let envelope_event = events
        .iter()
        .find(|e| e.tags.iter().any(|t| t == "tell"))
        .unwrap();
    assert_eq!(envelope_event.refs, vec![enhance.id.clone()]);

    // Unmounting the enhancer purges the chain: a second teller passes
    // through untransformed.
    mounter
        .unmount(mounter.silk_gate().admission("tr").unwrap().scope)
        .unwrap();
    let teller2 = mounter
        .mount(
            None,
            TELLER_YAML,
            json!({"name": "tl2", "to": "box"})
                .as_object()
                .unwrap()
                .clone(),
            &rights(),
            Some("tl2".into()),
            None,
        )
        .unwrap();
    let tl2_ctx = ActCtx {
        store: Some(&store),
        agent_id: "tl2",
        session: SESSION,
        card: None,
    };
    let policy2 = mounter.policy(teller2).unwrap();
    policy_step(
        &tl2_ctx,
        &NoModel,
        &policy2,
        &[],
        &ctx(),
        StepOpts {
            silk: Some(&mounter),
            ..StepOpts::default()
        },
    )
    .await
    .unwrap();
    let envelopes = silk_envelopes(&store);
    assert_eq!(envelopes.len(), 2);
    assert_eq!(envelopes[1].payload, json!({"text": "hello"}));
}
