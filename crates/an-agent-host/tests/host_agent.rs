//! A policy-sort spool through the host crate: the workspace config names
//! a persona, mount_from_config mounts it as an agent body, and a pump
//! drives the full chain — the body asks for a model call, core admits
//! and tapes it, the answer comes back as the reply, the landing is
//! taped as `applied`. The model client is the host's, scripted here.

#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};

use uuid::Uuid;

use an_agent_core::act::{Permit, ToolTag};
use an_agent_core::control::{AgentControl, EventRoute, ModelClient, ModelMessage, Registration};
use an_agent_core::memstream::Kind;
use an_agent_core::principal::card::{AgentCard, ModelSpec, ToolGrant, Topology};
use an_agent_core::testkit::{TempDir, bash_registry};
use an_agent_host::{Host, HostBodies, HostError};
use an_agent_spool::library;
use an_agent_spool::spool::Registry;

fn card() -> AgentCard {
    AgentCard {
        v: 1,
        id: Uuid::parse_str("dddddddd-dddd-dddd-dddd-dddddddddddd").unwrap(),
        model: ModelSpec {
            base_url: "https://example.com".into(),
            model: "m".into(),
            extra_body: None,
        },
        prompt: "you keep the tavern".into(),
        tools: vec![ToolGrant {
            name: "bash".into(),
            tag: ToolTag::none_permit(Permit::Deny),
        }],
        topology: Topology::Leaf,
        kernel: "0.1.0".into(),
        supersedes: None,
    }
}

/// A tavern persona as a policy: step 0 asks for a model call over the
/// newest clip, step 1 utters what the model answered.
const PERSONA: &str = r#"v: 1
kind: spool
name: persona
version: 1.0.0
summary: test persona, a policy-sort body
sort: policy
constructor: rhai
script: |
  let clips = params.clips;
  if params.steps == 0 {
    #{ kind: "invoke_model", clips: [clips[clips.len() - 1].id] }
  } else {
    #{ kind: "utter", text: config.persona + " says: " + clips[clips.len() - 1].content }
  }
effect:
  net: none
  file: { op: none }
  proc: none
  memory: { op: ignore }
  flow: none
inverse: none
requires: []
consumes: ["scene"]
produces: []
"#;

/// A gate is hook-chain machinery; mounting it must be refused.
const GATE: &str = r#"v: 1
kind: spool
name: bouncer
version: 1.0.0
summary: test gate, refused at mount
sort: gate
constructor: rhai
script: |
  #{ kind: "halt" }
effect:
  net: none
  file: { op: none }
  proc: none
  memory: { op: ignore }
  flow: none
inverse: none
requires: []
consumes: []
produces: []
"#;

#[derive(Default)]
struct StubModel {
    calls: Mutex<Vec<Vec<ModelMessage>>>,
}

impl ModelClient for StubModel {
    fn complete(&self, _spec: &ModelSpec, messages: Vec<ModelMessage>) -> Result<String, String> {
        self.calls
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(messages);
        Ok("welcome, traveler".into())
    }
}

#[test]
fn a_policy_persona_pumps_end_to_end() {
    let tmp = TempDir::new("host-agent");
    let shelf = tmp.path().join("shelf");
    std::fs::create_dir_all(&shelf).unwrap();
    std::fs::write(shelf.join("persona.yaml"), PERSONA).unwrap();
    std::fs::write(shelf.join("bouncer.yaml"), GATE).unwrap();

    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    control
        .register(&Registration {
            events: vec![EventRoute {
                name: "scene".into(),
                spools: vec!["aria".into()],
            }],
            config: vec!["main".into()],
            env: vec![],
        })
        .unwrap();
    control
        .put_config(
            "main",
            br#"
[[spool]]
name = "persona"
version = "1.0.0"
mount = "aria"
config = { persona = "Aria" }
"#,
        )
        .unwrap();

    let registry = Registry::open(tmp.path().join("registry")).unwrap();
    library::publish(&registry, &shelf.join("persona.yaml")).unwrap();
    library::publish(&registry, &shelf.join("bouncer.yaml")).unwrap();

    let host = Host::new(control, registry, HostBodies::default());
    assert_eq!(host.mount_from_config().unwrap(), vec!["aria"]);
    let model = Arc::new(StubModel::default());
    host.control().set_model_client(model.clone());

    let id = host.control().open_thread(&card()).unwrap();
    let scene = host
        .control()
        .note(id, "scene", "a traveler enters")
        .unwrap();
    host.control()
        .push_event("scene", "a traveler enters")
        .unwrap();

    let mut utters: Vec<String> = Vec::new();
    let outcomes = host
        .pump(id, &mut |text| utters.push(text.to_string()))
        .unwrap();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(utters.len(), 1);
    assert!(utters[0].starts_with("Aria says: "));
    assert!(utters[0].contains("welcome, traveler"));

    // The client saw the card's prompt in the composed envelope, plus
    // the cited scene clip.
    let calls = model.calls.lock().unwrap_or_else(|err| err.into_inner());
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0][0].role, "system");
    assert!(calls[0][0].content.starts_with("you keep the tavern"));
    assert!(calls[0][0].content.contains("# Host"));
    assert!(calls[0][1].content.contains("a traveler enters"));
    drop(calls);

    // The model call is taped as its own anchored pair, the spool note
    // carries the reply, and the pump's landing cites the note.
    let tape = host.control().events(id).unwrap();
    let action = tape
        .iter()
        .find(|event| event.kind == Kind::Action && event.tags.iter().any(|tag| tag == "invoke"))
        .expect("invoke action");
    assert!(action.refs.iter().any(|r| r == &scene));
    assert!(tape.iter().any(|event| event.kind == Kind::Observation
        && event.tags.iter().any(|tag| tag == "invoke")
        && event.refs.iter().any(|r| r == &action.id)
        && event.content.contains("welcome, traveler")));
    assert!(tape.iter().any(|event| {
        event.tags.iter().any(|tag| tag == "applied")
            && event
                .refs
                .iter()
                .any(|r| Some(r) == outcomes[0].reply().map(|reply| &reply.tape_id))
    }));
}

#[test]
fn a_gate_sort_spool_is_refused_at_mount() {
    let tmp = TempDir::new("host-gate-refused");
    let shelf = tmp.path().join("shelf");
    std::fs::create_dir_all(&shelf).unwrap();
    std::fs::write(shelf.join("bouncer.yaml"), GATE).unwrap();

    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    control
        .register(&Registration {
            events: vec![],
            config: vec!["main".into()],
            env: vec![],
        })
        .unwrap();
    control
        .put_config(
            "main",
            br#"
[[spool]]
name = "bouncer"
version = "1.0.0"
"#,
        )
        .unwrap();
    let registry = Registry::open(tmp.path().join("registry")).unwrap();
    library::publish(&registry, &shelf.join("bouncer.yaml")).unwrap();

    let host = Host::new(control, registry, HostBodies::default());
    assert!(matches!(
        host.mount_from_config(),
        Err(HostError::Body(reason)) if reason.contains("gate")
    ));
}

/// Two mounts of one policy body with different persona config answer in
/// turn on one scene — one body, many instances holds for policies too.
#[test]
fn one_policy_body_many_personas() {
    let tmp = TempDir::new("host-agent-parley");
    let shelf = tmp.path().join("shelf");
    std::fs::create_dir_all(&shelf).unwrap();
    std::fs::write(shelf.join("persona.yaml"), PERSONA).unwrap();

    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    control
        .register(&Registration {
            events: vec![EventRoute {
                name: "scene".into(),
                spools: vec!["aria".into(), "bob".into()],
            }],
            config: vec!["main".into()],
            env: vec![],
        })
        .unwrap();
    control
        .put_config(
            "main",
            br#"
[[spool]]
name = "persona"
version = "1.0.0"
mount = "aria"
config = { persona = "Aria" }

[[spool]]
name = "persona"
version = "1.0.0"
mount = "bob"
config = { persona = "Bob" }
"#,
        )
        .unwrap();
    let registry = Registry::open(tmp.path().join("registry")).unwrap();
    library::publish(&registry, &shelf.join("persona.yaml")).unwrap();

    let host = Host::new(control, registry, HostBodies::default());
    assert_eq!(host.mount_from_config().unwrap(), vec!["aria", "bob"]);
    let model = Arc::new(StubModel::default());
    host.control().set_model_client(model.clone());

    let id = host.control().open_thread(&card()).unwrap();
    host.control()
        .note(id, "scene", "a traveler enters")
        .unwrap();
    host.control()
        .push_event("scene", "a traveler enters")
        .unwrap();

    let mut utters: Vec<String> = Vec::new();
    let outcomes = host
        .pump(id, &mut |text| utters.push(text.to_string()))
        .unwrap();
    assert_eq!(outcomes.len(), 2);
    assert!(utters.iter().any(|text| text.starts_with("Aria says: ")));
    assert!(utters.iter().any(|text| text.starts_with("Bob says: ")));
    // Each persona's model call is taped separately: cost is never hidden.
    // Bob thinks after Aria, so his clip is her observation — the tape is
    // the shared scene, not a per-persona mailbox.
    let calls = model.calls.lock().unwrap_or_else(|err| err.into_inner());
    assert_eq!(calls.len(), 2);
    assert!(calls[0][1].content.contains("a traveler enters"));
    assert!(calls[1][1].content.contains("welcome, traveler"));
    drop(calls);
    let tape = host.control().events(id).unwrap();
    let invokes = tape
        .iter()
        .filter(|event| event.kind == Kind::Action && event.tags.iter().any(|tag| tag == "invoke"))
        .count();
    assert_eq!(invokes, 2);
}
