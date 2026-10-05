//! An inbox crosses a partition. The other partition does not see it.

use super::*;
use super::{NoModel, ctx, rights};

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
