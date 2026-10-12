//! Published policy, actual host pump, supervisor mail, and explicit replanning.

#![allow(clippy::unwrap_used)]

use uuid::Uuid;

use an_agent_core::act::{Permit, ToolTag};
use an_agent_core::control::{
    AgentControl, DeliveryOutcome, EventRoute, Registration, ReviewResolution,
};
use an_agent_core::principal::card::{AgentCard, ModelSpec, ToolGrant, Topology};
use an_agent_core::testkit::{TempDir, bash_registry};
use an_agent_host::{Host, HostBodies};
use an_agent_spool::{library, spool::Registry};

const WORKER: &str = r#"v: 1
kind: spool
name: worker
version: 1.0.0
summary: review checkpoint
sort: policy
constructor: rhai
script: |
  if params.event.body == "destructive" {
    #{ kind: "request_review", reason: "work may be destroyed" }
  } else {
    #{ kind: "utter", text: "safe alternative" }
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
consumes: [scene]
produces: []
"#;

fn card(id: &str) -> AgentCard {
    AgentCard {
        v: 1,
        id: Uuid::parse_str(id).unwrap(),
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

#[test]
fn pending_review_is_not_applied_and_main_can_schedule_a_safe_alternative() {
    let tmp = TempDir::new("host-supervision");
    let shelf = tmp.path().join("worker.yaml");
    std::fs::write(&shelf, WORKER).unwrap();
    let registry = Registry::open(tmp.path().join("registry")).unwrap();
    library::publish(&registry, &shelf).unwrap();
    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    control
        .register(&Registration {
            events: vec![EventRoute {
                name: "scene".into(),
                spools: vec!["worker".into()],
            }],
            config: vec!["main".into()],
            env: vec![],
        })
        .unwrap();
    control
        .put_config(
            "main",
            b"[[spool]]\nname = \"worker\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();
    let child = control
        .open_thread(&card("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"))
        .unwrap();
    let main = control
        .open_thread(&card("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"))
        .unwrap();
    control.set_supervisor(child, main).unwrap();
    let host = Host::new(control, registry, HostBodies::default());
    host.mount_from_config().unwrap();
    let event = host.control().push_event("scene", "destructive").unwrap();
    let mut utterances = Vec::new();
    let outcomes = host
        .pump_on(child, &event.id, &mut |text| {
            utterances.push(text.to_string())
        })
        .unwrap();
    let DeliveryOutcome::NeedsReview(request) = &outcomes[0] else {
        panic!("review expected")
    };
    assert!(utterances.is_empty());
    assert!(
        !host
            .control()
            .events(child)
            .unwrap()
            .iter()
            .any(|e| e.tags.iter().any(|tag| tag == "applied"))
    );
    assert_eq!(host.control().pending_reviews(main).unwrap().len(), 1);
    host.control()
        .advance(main, "choose a non-destructive plan")
        .unwrap();
    host.control()
        .resolve_review(
            main,
            &request.id,
            ReviewResolution::Retry {
                note: "choose a safe alternative".into(),
            },
        )
        .unwrap();
    assert!(utterances.is_empty());
    let event = host.control().push_event("scene", "safe").unwrap();
    let outcomes = host
        .pump_on(child, &event.id, &mut |text| {
            utterances.push(text.to_string())
        })
        .unwrap();
    assert!(outcomes[0].reply().is_some());
    assert_eq!(utterances, vec!["safe alternative"]);
    assert_eq!(
        host.control()
            .events(child)
            .unwrap()
            .iter()
            .filter(|e| e.tags.iter().any(|tag| tag == "applied"))
            .count(),
        1
    );
}
