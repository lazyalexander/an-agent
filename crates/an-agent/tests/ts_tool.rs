//! Can a TS tool sit behind the Tool trait? Two findings, codified:
//! 1. the descriptor validator rejects a `ts` constructor (registry gap);
//! 2. a stdio-JSON subprocess adapter runs it fine through act and tape.

// Helper fns here are not #[test] fns, so allow-*-in-tests does not reach them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[allow(dead_code)]
mod support;

use std::sync::Arc;

use an_agent::act::{ActCtx, Tool, ToolCall, ToolCtx, ToolTag, run_tool_act};
use an_agent::memstream::{JsonlStore, Kind};
use an_agent::tools::descriptor;
use serde_json::{Value, json};
use support::TempDir;
use support::ts_tool::SubprocessTool;

fn ctx() -> ToolCtx {
    let (_tx, rx) = tokio::sync::watch::channel(false);
    ToolCtx { signal: Some(rx) }
}

fn word_count_tool() -> Arc<dyn Tool> {
    SubprocessTool::bun_script(
        "word_count",
        "Count words and chars in text",
        json!({
            "type": "object",
            "properties": { "text": { "type": "string" } },
            "required": ["text"]
        }),
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../packages/ts-tools/word_count.ts"
        ),
        ToolTag::none(),
    )
}

#[test]
fn ts_constructor_rejected_by_descriptor() {
    // The registry contract admits rhai/mcp only — a ts descriptor fails
    // admission today. This test pins the gap, not a bug.
    let yaml = r#"
v: 1
name: word_count
version: 0.1.0
constructor: ts
summary: Count words
effect:
  net: none
  file: { op: none }
  proc: none
  memory: { op: ignore }
requires: []
"#;
    let err = descriptor::parse(yaml).unwrap_err().to_string();
    assert!(err.contains("unknown constructor"), "got: {err}");
}

#[tokio::test]
async fn ts_tool_runs_through_act_and_tape() {
    if std::process::Command::new("bun")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("bun not found; skipping ts tool probe");
        return;
    }
    let tmp = TempDir::new("ts-tool");
    let store = JsonlStore::open(tmp.path().join("memory.jsonl")).unwrap();
    let actx = ActCtx {
        store: Some(&store),
        agent_id: "ts-probe",
        session: "s",
        card: None,
    };
    let tool = word_count_tool();
    let call = ToolCall {
        id: "c1".into(),
        name: "word_count".into(),
        arguments: json!({ "text": "hello world from typescript" }).to_string(),
    };
    let tools: Vec<Arc<dyn Tool>> = vec![tool];
    let result = run_tool_act(&actx, &tools, &call, &ctx()).await.unwrap();
    let out: Value = serde_json::from_str(&result.message.content).unwrap();
    assert_eq!(out["words"], 4);
    assert_eq!(out["chars"], 27);

    let events = store.read_all().unwrap();
    let action = events
        .iter()
        .find(|e| {
            e.kind == Kind::Action
                && e.act.as_ref().and_then(|a| a.tool.as_deref()) == Some("word_count")
        })
        .expect("tool action on tape");
    let obs = events
        .iter()
        .find(|e| e.kind == Kind::Observation && e.refs.first() == Some(&action.id))
        .expect("observation refs action");
    assert_eq!(obs.content, result.message.content);
}
