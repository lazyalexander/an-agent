//! Tell, stamp, and admit. One sender, one reply.

use super::*;
use super::{NoModel, SESSION, ctx, rights, silk_envelopes};

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
pub(super) const MAILBOX_YAML: &str = r#"
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
pub(super) const DEAF_YAML: &str = r#"
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
pub(super) const COURIER_YAML: &str = r#"
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
