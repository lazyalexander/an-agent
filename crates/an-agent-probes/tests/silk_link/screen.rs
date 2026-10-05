//! An enhancer rewrites a payload. The tape records both hashes.

use super::tell::{COURIER_YAML, DEAF_YAML, MAILBOX_YAML};
use super::*;
use super::{NoModel, SESSION, ctx, rights, silk_envelopes};

/// The enhancer: sees the envelope, returns a new payload — nothing else
/// is reachable.
pub(super) const TRANSLATOR_YAML: &str = r#"
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
pub(super) const SCREENED_YAML: &str = r#"
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
pub(super) const HALF_YAML: &str = r#"
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
pub(super) const TELLER_YAML: &str = r#"
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
