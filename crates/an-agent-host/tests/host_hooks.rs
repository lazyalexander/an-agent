//! Hook chains through the host runner: spool handlers are rhai gates
//! from the shelf (compiled once, cached), MCP handlers fail closed
//! until the factory grows MCP. Three cases: a before-deny blocks the
//! delivery and is taped; an after-deny withholds the reply while the
//! utterance chain still gates; a config swap to an MCP hook denies the
//! next delivery loudly, never silently.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use uuid::Uuid;

use an_agent_core::act::{Permit, ToolTag};
use an_agent_core::control::{AgentControl, ControlError, EventRoute, Registration};
use an_agent_core::principal::card::{AgentCard, ModelSpec, ToolGrant, Topology};
use an_agent_core::testkit::{TempDir, bash_registry};
use an_agent_host::{Host, HostBodies, HostError, HostHooks};
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
  "Sue heard " + event.body
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
produces: []
"#;

/// One gate for both chains: before blocks "blocked" in the event body,
/// after blocks "secret" in the reply text.
const SCREEN: &str = r#"v: 1
kind: spool
name: screen
version: 1.0.0
summary: test gate
constructor: rhai
script: |
  if chain == "before" {
    if subject.body.contains("blocked") {
      #{ allow: false, reason: "blocked word" }
    } else {
      true
    }
  } else {
    if subject.reply.contains("secret") {
      #{ allow: false, reason: "secret in reply" }
    } else {
      true
    }
  }
effect:
  net: none
  file: { op: none }
  proc: none
  memory: { op: ignore }
  model: { op: none }
  flow: none
inverse: none
requires: []
consumes: []
produces: []
"#;

fn setup_host(tmp: &TempDir, shelf: &[(&str, &str)], config_tail: &str) -> (Host, Registry) {
    let shelf_dir = tmp.path().join("shelf");
    std::fs::create_dir_all(&shelf_dir).unwrap();
    for (name, body) in shelf {
        std::fs::write(shelf_dir.join(format!("{name}.yaml")), body).unwrap();
    }

    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    control
        .register(&Registration {
            events: vec![EventRoute {
                name: "user.word".into(),
                spools: vec!["greeter".into()],
            }],
            config: vec!["main".into()],
            env: vec![],
        })
        .unwrap();
    control
        .put_config(
            "main",
            format!(
                r#"
[[spool]]
name = "greeter"
version = "1.0.0"

{config_tail}
"#
            )
            .as_bytes(),
        )
        .unwrap();

    let registry = Registry::open(tmp.path().join("registry")).unwrap();
    for (name, _) in shelf {
        library::publish(&registry, &shelf_dir.join(format!("{name}.yaml"))).unwrap();
    }
    let host = Host::new(control, registry, HostBodies::default());
    let runner_registry = Registry::open(tmp.path().join("registry")).unwrap();
    (host, runner_registry)
}

const SPOOL_HOOKS: &str = r#"
[hooks]
before = [{ type = "spool", name = "screen", version = "1.0.0" }]
after = [{ type = "spool", name = "screen", version = "1.0.0" }]
"#;

#[test]
fn spool_gates_run_on_both_chains() {
    let tmp = TempDir::new("host-hooks-spool");
    let (host, runner_registry) = setup_host(
        &tmp,
        &[("greeter", GREETER), ("screen", SCREEN)],
        SPOOL_HOOKS,
    );
    assert_eq!(host.mount_from_config().unwrap(), vec!["greeter"]);
    host.control()
        .set_hook_runner(Arc::new(HostHooks::new(runner_registry)));
    let id = host.control().open_thread(&card()).unwrap();

    // A clean event delivers; the utterance reaches the sink.
    let mut utters: Vec<String> = Vec::new();
    host.control().push_event("user.word", "hello").unwrap();
    let replies = host
        .pump(id, &mut |text| utters.push(text.to_string()))
        .unwrap();
    assert_eq!(utters, vec!["Sue heard hello".to_string()]);
    assert_eq!(replies.len(), 1);

    // A before-deny blocks delivery; the pump surfaces HookDenied and
    // the denial is taped with the gate's reason.
    host.control()
        .push_event("user.word", "a blocked thing")
        .unwrap();
    let err = host.pump(id, &mut |_| {}).unwrap_err();
    assert!(matches!(
        err,
        HostError::Control(ControlError::HookDenied { ref chain, .. }) if chain == "before"
    ));
    let tape = host.control().events(id).unwrap();
    assert!(tape.iter().any(|event| {
        event.tags.iter().any(|tag| tag == "hook") && event.content.contains("blocked word")
    }));

    // An after-deny withholds the reply from the host; the spool note
    // and the denial are both on tape.
    host.control()
        .push_event("user.word", "a secret note")
        .unwrap();
    let replies = host.pump(id, &mut |_| {}).unwrap();
    assert!(replies.is_empty());
    let tape = host.control().events(id).unwrap();
    assert!(tape.iter().any(|event| {
        event.tags.iter().any(|tag| tag == "spool") && event.content.contains("secret")
    }));
    assert!(tape.iter().any(|event| {
        event.tags.iter().any(|tag| tag == "hook") && event.content.contains("secret in reply")
    }));
}

#[test]
fn an_mcp_hook_fails_closed_until_the_factory_grows_mcp() {
    let tmp = TempDir::new("host-hooks-mcp");
    let (host, runner_registry) = setup_host(
        &tmp,
        &[("greeter", GREETER), ("screen", SCREEN)],
        r#"
[hooks]
before = [{ type = "mcp", server = "guard", tool = "screen" }]
"#,
    );
    assert_eq!(host.mount_from_config().unwrap(), vec!["greeter"]);
    host.control()
        .set_hook_runner(Arc::new(HostHooks::new(runner_registry)));
    let id = host.control().open_thread(&card()).unwrap();

    host.control().push_event("user.word", "hello").unwrap();
    let err = host.pump(id, &mut |_| {}).unwrap_err();
    assert!(matches!(
        err,
        HostError::Control(ControlError::HookDenied { ref chain, .. }) if chain == "before"
    ));
    let tape = host.control().events(id).unwrap();
    assert!(tape.iter().any(|event| {
        event.tags.iter().any(|tag| tag == "hook") && event.content.contains("not implemented")
    }));
}

/// A length gate: the after subject's reply must stay within 20 chars.
const LENGTH: &str = r#"v: 1
kind: spool
name: length
version: 1.0.0
summary: test length gate
constructor: rhai
script: |
  if subject.reply.len() > 20 {
    #{ allow: false, reason: "overlong" }
  } else {
    true
  }
effect:
  net: none
  file: { op: none }
  proc: none
  memory: { op: ignore }
  model: { op: none }
  flow: none
inverse: none
requires: []
consumes: []
produces: []
"#;

const GUARD: &str = r#"
[[guard]]
rule = "replies under 20 characters"
gate = { type = "spool", name = "length", version = "1.0.0" }
"#;

/// The stage-1 criterion: a guard really stops an overlong utterance.
/// A short reply passes, a long one is withheld from the host, and the
/// denial tapes the rule, the gate's reason, and the guard chain name.
#[test]
fn a_guard_blocks_an_overlong_reply_and_tapes_the_rule() {
    let tmp = TempDir::new("host-guard-length");
    let (host, runner_registry) =
        setup_host(&tmp, &[("greeter", GREETER), ("length", LENGTH)], GUARD);
    assert_eq!(host.mount_from_config().unwrap(), vec!["greeter"]);
    host.control()
        .set_hook_runner(Arc::new(HostHooks::new(runner_registry)));
    let id = host.control().open_thread(&card()).unwrap();

    // "Sue heard hi" is 12 chars: under the bar, delivered.
    let mut utters: Vec<String> = Vec::new();
    host.control().push_event("user.word", "hi").unwrap();
    let replies = host
        .pump(id, &mut |text| utters.push(text.to_string()))
        .unwrap();
    assert_eq!(utters, vec!["Sue heard hi".to_string()]);
    assert_eq!(replies.len(), 1);

    // "Sue heard a somewhat longer message" is over the bar: withheld.
    host.control()
        .push_event("user.word", "a somewhat longer message")
        .unwrap();
    let replies = host.pump(id, &mut |_| {}).unwrap();
    assert!(replies.is_empty());
    let tape = host.control().events(id).unwrap();
    assert!(tape.iter().any(|event| {
        event.tags.iter().any(|tag| tag == "spool") && event.content.contains("somewhat longer")
    }));
    assert!(tape.iter().any(|event| {
        event.tags.iter().any(|tag| tag == "hook")
            && event.content.contains("\"chain\":\"guard\"")
            && event.content.contains("replies under 20 characters")
            && event.content.contains("overlong")
    }));

    // offer surfaces the denial under the guard chain name.
    host.control()
        .push_event("user.word", "another message that runs long")
        .unwrap();
    assert!(matches!(
        host.control().offer(id, "greeter"),
        Err(ControlError::HookDenied { ref chain, .. }) if chain == "guard"
    ));
}

/// A guard whose gate is not on the shelf — or is an mcp gate before the
/// factory grows MCP — fails the mount pass, before anything mounts.
#[test]
fn a_guard_that_cannot_bind_fails_the_mount() {
    let tmp = TempDir::new("host-guard-missing");
    let (host, _) = setup_host(
        &tmp,
        &[("greeter", GREETER)],
        r#"
[[guard]]
rule = "replies under 20 characters"
gate = { type = "spool", name = "missing", version = "9.9.9" }
"#,
    );
    assert!(host.mount_from_config().is_err());

    let tmp = TempDir::new("host-guard-mcp");
    let (host, _) = setup_host(
        &tmp,
        &[("greeter", GREETER)],
        r#"
[[guard]]
rule = "no secrets"
gate = { type = "mcp", server = "guard", tool = "screen" }
"#,
    );
    let err = host.mount_from_config().unwrap_err();
    assert!(err.to_string().contains("mcp"), "{err}");
}
