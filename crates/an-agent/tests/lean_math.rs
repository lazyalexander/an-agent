//! Rhai script is the control flow: it yields the next Lean snippet (or
//! halt). The host admits each `lean_check` through `run_tool_act`. Covers
//! prelude arithmetic identities plus one deliberate type error.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[allow(dead_code)]
mod support;

use std::sync::Arc;

use an_agent::act::{ActCtx, Tool, ToolCall, ToolCtx, run_tool_act};
use an_agent::memstream::{JsonlStore, Kind};
use serde_json::{Value, json};
use support::TempDir;
use support::lean::LeanTool;

const SCRIPT: &str = include_str!("fixtures/lean_math.rhai");
const MAX_STEPS: i64 = 20;

fn ctx() -> ToolCtx {
    let (_tx, rx) = tokio::sync::watch::channel(false);
    ToolCtx { signal: Some(rx) }
}

fn continuation(step: i64, last_ok: bool, last_diag: &str) -> Value {
    let mut engine = rhai::Engine::new();
    engine.set_max_operations(100_000);
    engine.set_max_string_size(1 << 20);
    engine.set_max_array_size(10_000);
    engine.set_max_map_size(10_000);
    let mut scope = rhai::Scope::new();
    let args = json!({
        "step": step,
        "last_ok": last_ok,
        "last_diag": last_diag,
    });
    scope.push(
        "params",
        rhai::serde::to_dynamic(args).expect("params to dynamic"),
    );
    let out = engine
        .eval_with_scope::<rhai::Dynamic>(&mut scope, SCRIPT)
        .unwrap_or_else(|e| panic!("rhai control flow failed at step {step}: {e}"));
    rhai::serde::from_dynamic(&out).expect("continuation json")
}

fn diag(payload: &Value) -> String {
    format!(
        "{}{}",
        payload["stdout"].as_str().unwrap_or(""),
        payload["stderr"].as_str().unwrap_or("")
    )
}

#[tokio::test]
#[ignore = "needs lean4"]
async fn rhai_drives_lean_math_problems() {
    let tool: Arc<dyn Tool> = LeanTool::discover().expect("lean4");
    let tmp = TempDir::new("lean-math");
    let store = JsonlStore::open(tmp.path().join("memory.jsonl")).unwrap();
    let actx = ActCtx {
        store: Some(&store),
        agent_id: "lean-math",
        session: "s",
        card: None,
    };
    let tools = vec![tool];

    let mut step = 0_i64;
    let mut last_ok = true;
    let mut last_diag = String::new();
    let mut solved = 0_u32;

    loop {
        assert!(step < MAX_STEPS, "control flow did not halt");
        let cont = continuation(step, last_ok, &last_diag);
        let op = cont["op"].as_str().unwrap_or("");
        if op == "halt" {
            break;
        }
        assert_eq!(op, "lean", "unknown continuation: {cont}");
        let id = cont["id"].as_str().expect("id");
        let source = cont["source"].as_str().expect("source");
        let expect = cont["expect"].as_bool().expect("expect");

        let call = ToolCall {
            id: format!("c{step}"),
            name: "lean_check".into(),
            arguments: json!({ "source": source }).to_string(),
        };
        let result = run_tool_act(&actx, &tools, &call, &ctx()).await.unwrap();
        let payload: Value = serde_json::from_str(&result.message.content)
            .unwrap_or_else(|_| json!({ "raw": result.message.content }));
        let ok = payload["ok"].as_bool().unwrap_or(false);
        last_diag = diag(&payload);
        last_ok = ok;
        assert_eq!(
            ok, expect,
            "problem {id}: expected ok={expect}, got {payload}"
        );
        if expect {
            solved += 1;
        }
        step += 1;
    }

    assert_eq!(solved, 7, "seven identities should check");
    assert_eq!(step, 8, "seven proofs plus one counterexample");

    let events = store.read_all().unwrap();
    let actions: Vec<_> = events
        .iter()
        .filter(|e| {
            e.kind == Kind::Action
                && e.act.as_ref().and_then(|a| a.tool.as_deref()) == Some("lean_check")
        })
        .collect();
    assert_eq!(actions.len(), 8);
    for a in &actions {
        assert!(
            events
                .iter()
                .any(|e| e.kind == Kind::Observation && e.refs.first() == Some(&a.id)),
            "observation must ref {}",
            a.id
        );
    }
}
