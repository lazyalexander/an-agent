//! Lean 4 behind the Tool trait. Control flow is this test script, not rhai:
//! admit a theorem, admit a type error, admit `lake build` of a tiny package.
//! Each step goes through `run_tool_act`; the tape must show the chain.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[allow(dead_code)]
mod support;

use std::sync::Arc;

use an_agent::act::{ActCtx, Tool, ToolCall, ToolCtx, run_tool_act};
use an_agent::memstream::{JsonlStore, Kind};
use serde_json::{Value, json};
use support::TempDir;
use support::lean::LeanTool;

fn ctx() -> ToolCtx {
    let (_tx, rx) = tokio::sync::watch::channel(false);
    ToolCtx { signal: Some(rx) }
}

fn fixture_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packages/lean-probe")
}

fn copy_lake_fixture(dst: &std::path::Path) {
    let src = fixture_dir();
    for name in ["lean-toolchain", "lakefile.toml", "LeanProbe.lean"] {
        std::fs::copy(src.join(name), dst.join(name)).unwrap_or_else(|e| {
            panic!("copy {name}: {e}");
        });
    }
}

async fn check(
    actx: &ActCtx<'_>,
    tools: &[Arc<dyn Tool>],
    id: &str,
    args: Value,
) -> (String, Value) {
    let call = ToolCall {
        id: id.into(),
        name: "lean_check".into(),
        arguments: args.to_string(),
    };
    let result = run_tool_act(actx, tools, &call, &ctx()).await.unwrap();
    let body = result.message.content;
    let parsed: Value =
        serde_json::from_str(&body).unwrap_or_else(|_| json!({ "raw": body.clone() }));
    (body, parsed)
}

#[tokio::test]
#[ignore = "needs lean4"]
async fn lean4_compat_script() {
    let tool: Arc<dyn Tool> = LeanTool::discover()
        .expect("lean4")
        .with_timeout(std::time::Duration::from_secs(120));
    let tmp = TempDir::new("lean4");
    let store = JsonlStore::open(tmp.path().join("memory.jsonl")).unwrap();
    let actx = ActCtx {
        store: Some(&store),
        agent_id: "lean-probe",
        session: "s",
        card: None,
    };
    let tools = vec![tool];

    // 1. a closed proof
    let (_, ok) = check(
        &actx,
        &tools,
        "c1",
        json!({ "source": "example : 1 + 1 = 2 := rfl\n" }),
    )
    .await;
    assert_eq!(ok["ok"], true, "valid snippet: {ok}");

    // 2. a type error — still an admitted act, payload says not ok
    let (_, bad) = check(
        &actx,
        &tools,
        "c2",
        json!({ "source": "example : 1 + 1 = 3 := rfl\n" }),
    )
    .await;
    assert_eq!(bad["ok"], false, "type error should not be ok: {bad}");
    let diag = format!(
        "{}{}",
        bad["stdout"].as_str().unwrap_or(""),
        bad["stderr"].as_str().unwrap_or("")
    );
    assert!(
        diag.contains("Type mismatch") || diag.contains("type mismatch"),
        "expected a type error, got {bad}"
    );

    // 3. academic layout: Lake package copied off-tree, then `lake build`
    let proj = tmp.path().join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    copy_lake_fixture(&proj);
    let (_, lake) = check(
        &actx,
        &tools,
        "c3",
        json!({ "project": proj.to_string_lossy() }),
    )
    .await;
    assert_eq!(lake["ok"], true, "lake build: {lake}");

    let events = store.read_all().unwrap();
    let actions: Vec<_> = events
        .iter()
        .filter(|e| {
            e.kind == Kind::Action
                && e.act.as_ref().and_then(|a| a.tool.as_deref()) == Some("lean_check")
        })
        .collect();
    assert_eq!(actions.len(), 3, "three admitted lean_check acts");
    for a in &actions {
        let obs = events
            .iter()
            .find(|e| e.kind == Kind::Observation && e.refs.first() == Some(&a.id));
        assert!(obs.is_some(), "observation must ref action {}", a.id);
    }
}
