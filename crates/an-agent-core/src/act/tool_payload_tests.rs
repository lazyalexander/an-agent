//! Tool bodies stay private while legacy records and live messages remain usable.
use super::tool::*;
use crate::{
    act::{MemoryFacet, ToolTag},
    memstream::{Kind, TOOL_PAYLOAD_LIMIT},
    testkit::TempDir,
};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
struct Echo {
    calls: Arc<AtomicUsize>,
    remember: bool,
    output: String,
}
#[async_trait::async_trait]
impl Tool for Echo {
    fn name(&self) -> &str {
        "echo"
    }
    fn description(&self) -> &str {
        "test"
    }
    fn parameters(&self) -> serde_json::Value {
        json!({})
    }
    fn tag_seed(&self) -> Option<ToolTag> {
        let mut tag = ToolTag::none();
        if self.remember {
            tag.memory = MemoryFacet::Remember { aspect: None };
        }
        Some(tag)
    }
    async fn execute(&self, _: serde_json::Value, _: &ToolCtx) -> Result<String, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.output.clone())
    }
}
#[tokio::test]
async fn private_bodies_reopen_verify_and_do_not_leak_into_tape() {
    for remember in [false, true] {
        let dir = TempDir::new("tool-payload");
        let path = dir.path().join("memory.jsonl");
        let store = crate::memstream::JsonlStore::open(&path).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let tools: Vec<Arc<dyn Tool>> = vec![Arc::new(Echo {
            calls: calls.clone(),
            remember,
            output: "private result".into(),
        })];
        let call = ToolCall {
            id: "call".into(),
            name: "echo".into(),
            arguments: r#"{"secret":"private argument"}"#.into(),
        };
        let result = run_tool_act(
            &ActCtx {
                store: Some(&store),
                agent_id: "a",
                session: "s",
                card: None,
            },
            &tools,
            &call,
            &ToolCtx { signal: None },
        )
        .await
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        if !remember {
            assert_eq!(result.message.content, "private result");
        }
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("private argument"));
        assert!(!raw.contains("private result"));
        drop(store);
        let store = crate::memstream::JsonlStore::open(&path).unwrap();
        let events = store.read_all().unwrap();
        assert_eq!(
            store.resolve_tool_payload(&events[0]).unwrap(),
            call.arguments
        );
        let result = events
            .iter()
            .find(|e| {
                if remember {
                    e.kind == Kind::Utterance
                } else {
                    e.kind == Kind::Observation
                }
            })
            .unwrap();
        assert_eq!(
            store.resolve_tool_payload(result).unwrap(),
            "private result"
        );
        let reference: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        let blob = dir
            .path()
            .join("blobs")
            .join(reference["sha256"].as_str().unwrap());
        std::fs::write(&blob, "corrupted").unwrap();
        assert!(store.resolve_tool_payload(result).is_err());
        std::fs::remove_file(blob).unwrap();
        assert!(store.resolve_tool_payload(result).is_err());
        let mut legacy = result.clone();
        legacy.tags.clear();
        legacy.content = "literal legacy".into();
        assert_eq!(
            store.resolve_tool_payload(&legacy).unwrap(),
            "literal legacy"
        );
    }
}
#[tokio::test]
async fn oversized_input_never_executes_and_output_leaves_explicit_rejection() {
    let dir = TempDir::new("tool-bounds");
    let store = crate::memstream::JsonlStore::open(dir.path().join("memory.jsonl")).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let tools: Vec<Arc<dyn Tool>> = vec![Arc::new(Echo {
        calls: calls.clone(),
        remember: true,
        output: "x".repeat(TOOL_PAYLOAD_LIMIT + 1),
    })];
    let act = ActCtx {
        store: Some(&store),
        agent_id: "a",
        session: "s",
        card: None,
    };
    let mut call = ToolCall {
        id: "call".into(),
        name: "echo".into(),
        arguments: "x".repeat(TOOL_PAYLOAD_LIMIT + 1),
    };
    assert!(
        run_tool_act(&act, &tools, &call, &ToolCtx { signal: None })
            .await
            .is_err()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(store.read_all().unwrap().is_empty());
    call.arguments = "{}".into();
    assert!(
        run_tool_act(&act, &tools, &call, &ToolCtx { signal: None })
            .await
            .is_err()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let events = store.read_all().unwrap();
    assert_eq!(events.len(), 2);
    assert!(
        store
            .resolve_tool_payload(&events[1])
            .unwrap()
            .contains("effects may have occurred")
    );
}
