//! The parley pattern through the host crate: one rhai body mounted
//! twice as two personas, one native body from the app's HostBodies.
//! The host mounts everything from workspace config, then pumps: replies
//! become utterances at the app's sink and raised events on the log, and
//! every landing is taped as `applied`. The parley case's hand-rolled
//! glue is what this crate absorbs.

#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};

use uuid::Uuid;

use an_agent_core::act::{Permit, ToolTag};
use an_agent_core::control::{AgentControl, EventRoute, Registration, SpoolBeat};
use an_agent_core::principal::card::{AgentCard, ModelSpec, ToolGrant, Topology};
use an_agent_core::testkit::{TempDir, bash_registry};
use an_agent_core::workspace::WorkspaceRecord;
use an_agent_host::{Host, HostBodies};
use an_agent_spool::library;
use an_agent_spool::spool::Registry;

fn card() -> AgentCard {
    AgentCard {
        v: 1,
        id: Uuid::parse_str("cccccccc-cccc-cccc-cccc-cccccccccccc").unwrap(),
        model: ModelSpec {
            base_url: "https://example.com".into(),
            model: "m".into(),
            extra_body: None,
        },
        prompt: "p".into(),
        tools: vec![ToolGrant {
            name: "bash".into(),
            tag: ToolTag::none_permit(Permit::Deny),
        }],
        topology: Topology::Leaf,
        kernel: "0.1.0".into(),
        supersedes: None,
    }
}

const GREETER: &str = r#"v: 1
kind: spool
name: greeter
version: 1.0.0
summary: test greeter
constructor: rhai
script: |
  let utter = config.persona + " heard " + event.body;
  let done = "said-by-" + config.persona;
  "{\"utter\":\"" + utter + "\",\"raise\":[{\"event\":\"greeter.done\",\"body\":\"" + done + "\"}]}"
effect:
  net: none
  file: { op: none }
  proc: none
  memory: { op: ignore }
  model: { op: none }
  flow: none
inverse: none
requires: []
consumes: ["user.word"]
produces: ["greeter.done"]
"#;

const OUTBOX: &str = r#"v: 1
kind: spool
name: outbox
version: 1.0.0
summary: test outbox, a host-constructed body
constructor: host
host: outbox
effect:
  net: none
  file: { op: none }
  proc: none
  memory: { op: ignore }
  model: { op: none }
  flow: none
inverse: none
requires: []
consumes: ["greeter.done"]
produces: []
"#;

/// The app's native body: records what it receives, replies "recorded".
struct Outbox(Arc<Mutex<Vec<String>>>);

impl SpoolBeat for Outbox {
    fn receive(&self, event: &WorkspaceRecord) -> Result<String, String> {
        self.0
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(event.body.clone().unwrap_or_default());
        Ok("recorded".into())
    }
}

#[test]
fn mount_from_config_then_pump_the_parley() {
    let tmp = TempDir::new("host-pump");
    let shelf = tmp.path().join("shelf");
    std::fs::create_dir_all(&shelf).unwrap();
    std::fs::write(shelf.join("greeter.yaml"), GREETER).unwrap();
    std::fs::write(shelf.join("outbox.yaml"), OUTBOX).unwrap();

    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    control
        .register(&Registration {
            events: vec![
                EventRoute {
                    name: "user.word".into(),
                    spools: vec!["annie".into(), "bob".into()],
                },
                EventRoute {
                    name: "greeter.done".into(),
                    spools: vec!["outbox".into()],
                },
            ],
            config: vec!["main".into()],
            env: vec![],
        })
        .unwrap();
    control
        .put_config(
            "main",
            br#"
[[spool]]
name = "greeter"
version = "1.0.0"
mount = "annie"
config = { persona = "Annie" }

[[spool]]
name = "greeter"
version = "1.0.0"
mount = "bob"
config = { persona = "Bob" }

[[spool]]
name = "outbox"
version = "1.0.0"
"#,
        )
        .unwrap();

    let registry = Registry::open(tmp.path().join("registry")).unwrap();
    library::publish(&registry, &shelf.join("greeter.yaml")).unwrap();
    library::publish(&registry, &shelf.join("outbox.yaml")).unwrap();

    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen_in_body = Arc::clone(&seen);
    let mut bodies = HostBodies::default();
    bodies.register("outbox", move |_config| {
        Ok(Outbox(Arc::clone(&seen_in_body)))
    });
    let host = Host::new(control, registry, bodies);

    // One body, two instances: personas are mount config, not code.
    assert_eq!(
        host.mount_from_config().unwrap(),
        vec!["annie", "bob", "outbox"]
    );

    let id = host.control().open_thread(&card()).unwrap();
    host.control()
        .push_event("user.word", "alice: land ho!")
        .unwrap();

    // Pump one: both personas utter and raise; the raises land as events.
    let mut utters: Vec<String> = Vec::new();
    let outcomes = host
        .pump(id, &mut |text| utters.push(text.to_string()))
        .unwrap();
    assert_eq!(
        utters,
        vec![
            "Annie heard alice: land ho!".to_string(),
            "Bob heard alice: land ho!".to_string(),
        ]
    );
    let dones: Vec<_> = host
        .control()
        .workspace_log()
        .unwrap()
        .into_iter()
        .filter(|record| record.name.as_deref() == Some("greeter.done"))
        .map(|record| (record.id.clone(), record.body.clone().unwrap()))
        .collect();
    assert_eq!(dones.len(), 2);
    // Both landings taped, citing their spool notes.
    let tape = host.control().events(id).unwrap();
    for reply in outcomes.iter().filter_map(|o| o.reply()) {
        assert!(tape.iter().any(|event| {
            event.tags.iter().any(|tag| tag == "applied")
                && event.refs.iter().any(|r| r == &reply.tape_id)
        }));
    }

    // Pump two: the latest raise goes to the native body; its reply is
    // a plain string, hence one utterance.
    host.pump(id, &mut |text| utters.push(text.to_string()))
        .unwrap();
    assert_eq!(
        seen.lock()
            .unwrap_or_else(|err| err.into_inner())
            .as_slice(),
        &["said-by-Bob".to_string()]
    );
    assert_eq!(utters.last(), Some(&"recorded".to_string()));

    // Backlog: the first raise is still pending; pump it by id.
    host.pump_on(id, &dones[0].0, &mut |text| utters.push(text.to_string()))
        .unwrap();
    assert_eq!(
        seen.lock()
            .unwrap_or_else(|err| err.into_inner())
            .as_slice(),
        &["said-by-Bob".to_string(), "said-by-Annie".to_string()]
    );
}
