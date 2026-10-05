//! Continuation parsing strictness and the policy construction gate.

use super::*;

fn spec(effect: &str) -> SpoolSpec {
    let yaml = format!(
        "v: 1\nkind: spool\nname: pol\nversion: 1.0.0\nsummary: p\nconstructor: rhai\nscript: |\n  #{{ kind: \"halt\" }}\neffect:\n{effect}\n  flow: none\ninverse: none\nrequires:\n  - {{ name: echo, version: 1.0.0 }}\nconsumes: []\nproduces: []\n"
    );
    crate::spool::parse(&yaml).unwrap()
}

#[test]
fn continuation_parsing_is_strict() {
    use serde_json::json;
    assert_eq!(
        Continuation::from_value(&json!({"kind": "halt"})).unwrap(),
        Continuation::Halt
    );
    assert_eq!(
        Continuation::from_value(&json!({"kind": "utter", "text": "hi"})).unwrap(),
        Continuation::Utter { text: "hi".into() }
    );
    assert_eq!(
        Continuation::from_value(&json!({"kind": "invoke_model", "clips": ["a", "b"]})).unwrap(),
        Continuation::InvokeModel {
            clips: vec!["a".into(), "b".into()]
        }
    );
    assert_eq!(
        Continuation::from_value(
            &json!({"kind": "invoke_tool", "name": "echo", "args": {"text": "x"}})
        )
        .unwrap(),
        Continuation::InvokeTool {
            name: "echo".into(),
            args: json!({"text": "x"})
        }
    );
    assert_eq!(
        Continuation::from_value(&json!({"kind": "approve", "clip": "c1", "index": 0})).unwrap(),
        Continuation::Approve {
            clip: "c1".into(),
            index: 0
        }
    );
    // Garbage is a hard error, never reinterpreted.
    assert!(Continuation::from_value(&json!({"kind": "nope"})).is_err());
    assert!(Continuation::from_value(&json!({"kind": "utter"})).is_err());
    assert!(Continuation::from_value(&json!({"clips": []})).is_err());
    assert!(Continuation::from_value(&json!({"kind": "invoke_model", "clips": [1]})).is_err());
    assert!(
        Continuation::from_value(&json!({"kind": "invoke_tool", "name": "x", "args": 1})).is_err()
    );
    assert!(Continuation::from_value(&json!({"kind": "approve", "clip": "c"})).is_err());
    // Canonical form round-trips through the parser.
    let cont = Continuation::InvokeTool {
        name: "echo".into(),
        args: json!({}),
    };
    assert_eq!(Continuation::from_value(&cont.to_value()).unwrap(), cont);
}

#[test]
fn policy_rejects_effector_faces_and_keeps_requires_whitelist() {
    let net = spec("  net: egress\n  file: { op: none }\n  proc: none\n  memory: { op: ignore }");
    assert!(
        RhaiPolicy::from_spool(&net, serde_json::Map::new())
            .err()
            .unwrap()
            .contains("net")
    );
    let file = spec(
        "  net: none\n  file: { op: r, path: \"/srv\" }\n  proc: none\n  memory: { op: ignore }",
    );
    assert!(
        RhaiPolicy::from_spool(&file, serde_json::Map::new())
            .err()
            .unwrap()
            .contains("file face")
    );
    let spawn = spec("  net: none\n  file: { op: none }\n  proc: spawn\n  memory: { op: ignore }");
    assert!(
        RhaiPolicy::from_spool(&spawn, serde_json::Map::new())
            .err()
            .unwrap()
            .contains("proc")
    );
    // A pure policy constructs; requires become the invoke whitelist.
    let ok = spec(
        "  net: none\n  file: { op: none }\n  proc: none\n  memory: { op: remember, aspect: ctx }",
    );
    let policy = RhaiPolicy::from_spool(&ok, serde_json::Map::new()).unwrap();
    assert_eq!(policy.whitelist(), vec!["echo".to_string()]);
}

#[test]
fn evaluate_yields_a_validated_continuation() {
    let ok = spec("  net: none\n  file: { op: none }\n  proc: none\n  memory: { op: ignore }");
    let policy = RhaiPolicy::from_spool(&ok, serde_json::Map::new()).unwrap();
    assert_eq!(
        policy.evaluate(serde_json::json!({})).unwrap(),
        Continuation::Halt
    );
    // A script yielding garbage fails at the boundary.
    let garbage = crate::spool::parse(
        "v: 1\nkind: spool\nname: bad\nversion: 1.0.0\nsummary: b\nconstructor: rhai\nscript: |\n  42\neffect:\n  net: none\n  file: { op: none }\n  proc: none\n  memory: { op: ignore }\n  flow: none\ninverse: none\nrequires: []\nconsumes: []\nproduces: []\n",
    )
    .unwrap();
    let policy = RhaiPolicy::from_spool(&garbage, serde_json::Map::new()).unwrap();
    assert!(policy.evaluate(serde_json::json!({})).is_err());
}
