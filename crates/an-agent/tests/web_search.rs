//! web_search via rhai descriptor: YAML admission → effect-gated host
//! functions → tape. The gating test is offline (the whitelist IS the
//! wiring); the live test hits the real web and tapes the act.

// Helper fns here are not #[test] fns, so allow-*-in-tests does not reach them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[allow(dead_code)]
mod support;

use std::path::PathBuf;
use std::sync::Arc;

use an_agent::act::{ActCtx, Tool, ToolCall, ToolCtx, run_tool_act};
use an_agent::memstream::{JsonlStore, Kind};
use an_agent::tools::descriptor::{self, Constructor, Net};
use serde_json::{Value, json};
use support::TempDir;
use support::rhai::RhaiTool;

const YAML: &str = include_str!("fixtures/web_search.yaml");

fn params_schema() -> Value {
    json!({
        "type": "object",
        "properties": { "query": { "type": "string", "description": "search query" } },
        "required": ["query"]
    })
}

fn ctx() -> ToolCtx {
    let (_tx, rx) = tokio::sync::watch::channel(false);
    ToolCtx { signal: Some(rx) }
}

fn probe_dir() -> (PathBuf, Option<TempDir>) {
    if let Ok(dir) = std::env::var("AN_AGENT_PROBE_DIR") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("create probe dir");
        return (dir, None);
    }
    let tmp = TempDir::new("websearch");
    (tmp.path().to_path_buf(), Some(tmp))
}

#[test]
fn descriptor_admits() {
    let d = descriptor::parse(YAML).unwrap();
    assert_eq!(d.name, "web_search");
    assert_eq!(d.version, "0.1.0");
    assert!(matches!(d.constructor, Constructor::Rhai { .. }));
    assert_eq!(d.effect.net, Net::Egress);
    assert!(d.requires.is_empty());
}

/// Tamper the declared effect to net: none and the script's http_get_json
/// call must fail — the whitelist is the wiring, not a promise.
#[tokio::test]
async fn effect_gates_host_functions() {
    let mut d = descriptor::parse(YAML).unwrap();
    d.effect.net = Net::None;
    let tool = RhaiTool::from_descriptor(
        d,
        params_schema(),
        &["api.duckduckgo.com", "en.wikipedia.org"],
    )
    .unwrap();
    let actx = ActCtx {
        store: None,
        agent_id: "gate-probe",
        session: "s",
        card: None,
    };
    let call = ToolCall {
        id: "c1".into(),
        name: "web_search".into(),
        arguments: r#"{"query":"rust"}"#.into(),
    };
    let tools: Vec<Arc<dyn Tool>> = vec![Arc::new(tool)];
    let result = run_tool_act(&actx, &tools, &call, &ctx()).await.unwrap();
    assert!(
        result.message.content.contains("http_get_json"),
        "expected unknown-function failure, got: {}",
        result.message.content
    );
}

/// Live: real web through the rhai tool, taped via run_tool_act.
#[tokio::test]
#[ignore = "live web probe; needs network"]
async fn web_search_live() {
    let d = descriptor::parse(YAML).unwrap();
    let tool = RhaiTool::from_descriptor(
        d,
        params_schema(),
        &["api.duckduckgo.com", "en.wikipedia.org"],
    )
    .unwrap();
    let (dir, _guard) = probe_dir();
    eprintln!("probe dir: {}", dir.display());
    let store = JsonlStore::open(dir.join("memory.jsonl")).unwrap();
    // A probe dir may hold earlier runs; scope reads to this run's session.
    let session = an_agent::det_seam::Entropy::os().uuid_v4().to_string();
    let actx = ActCtx {
        store: Some(&store),
        agent_id: "websearch-probe",
        session: &session,
        card: None,
    };
    let call = ToolCall {
        id: "c1".into(),
        name: "web_search".into(),
        arguments: json!({ "query": "Rust programming language creator" }).to_string(),
    };
    let tools: Vec<Arc<dyn Tool>> = vec![Arc::new(tool)];
    let result = run_tool_act(&actx, &tools, &call, &ctx()).await.unwrap();
    eprintln!("=== digest ===\n{}", result.message.content);
    let c = &result.message.content;
    assert!(!c.is_empty());
    // Error lines alone mean both backends failed — a failed probe must not
    // pass. Success digests never carry the "<backend>: <error>" prefix.
    assert!(
        !c.contains("duckduckgo:") && !c.contains("wikipedia:"),
        "search backends failed, digest is only errors: {c}"
    );

    let events = store.read_all().unwrap();
    let mine: Vec<_> = events
        .iter()
        .filter(|e| e.session.as_deref() == Some(session.as_str()))
        .collect();
    let action = mine
        .iter()
        .find(|e| {
            e.kind == Kind::Action
                && e.act.as_ref().and_then(|a| a.tool.as_deref()) == Some("web_search")
        })
        .expect("tool action on tape");
    let obs = mine
        .iter()
        .find(|e| e.kind == Kind::Observation && e.refs.first() == Some(&action.id))
        .expect("observation refs action");
    assert_eq!(obs.content, result.message.content);
    assert_eq!(action.act.as_ref().unwrap().kind, "invoke");
}
