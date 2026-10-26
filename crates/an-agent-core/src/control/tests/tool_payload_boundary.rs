//! Public control paths must bound storage and preserve exact selected evidence.
use super::*;
use crate::{
    act::{MemoryFacet, ToolCall, ToolError},
    memstream::{Kind, TOOL_PAYLOAD_LIMIT},
};
use std::{collections::BTreeMap, fs, path::Path};

fn blobs(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let path = dir.join("blobs");
    if !path.exists() {
        return BTreeMap::new();
    }
    fs::read_dir(path)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            (
                e.file_name().to_string_lossy().into_owned(),
                fs::read(e.path()).unwrap(),
            )
        })
        .collect()
}

#[tokio::test]
async fn oversized_calls_leave_review_tapes_and_cas_untouched() {
    for permit in [Permit::Go, Permit::Ask, Permit::Deny] {
        let (_tmp, control, child, parent) = super::supervision::setup(permit);
        let directory = control.directory(child).unwrap();
        let child_before = serde_json::to_vec(&control.events(child).unwrap()).unwrap();
        let parent_before = serde_json::to_vec(&control.events(parent).unwrap()).unwrap();
        let before = blobs(&directory);
        for field in ["arguments", "name", "id"] {
            let mut call = super::supervision::call(1);
            match field {
                "arguments" => call.arguments = "x".repeat(TOOL_PAYLOAD_LIMIT + 1),
                "name" => call.name = "x".repeat(257),
                _ => call.id = "x".repeat(257),
            }
            assert!(matches!(
                control.run(child, &call).await,
                Err(ControlError::Tool(ToolError::PayloadLimit(_)))
            ));
            assert_eq!(blobs(&directory), before);
            assert_eq!(
                serde_json::to_vec(&control.events(child).unwrap()).unwrap(),
                child_before
            );
            assert_eq!(
                serde_json::to_vec(&control.events(parent).unwrap()).unwrap(),
                parent_before
            );
        }
        // Rejection did not leave a beat held or disable the normal review path.
        let valid = control.run(child, &super::supervision::call(1)).await;
        assert_eq!(valid.is_ok(), permit == Permit::Go);
    }
}

struct Select {
    ids: Vec<String>,
    seen: Mutex<Vec<Value>>,
}
impl AgentBody for Select {
    fn evaluate(&self, projection: &Value) -> Result<BeatStep, String> {
        self.seen.lock().unwrap().push(projection.clone());
        Ok(if projection["steps"] == 0 {
            BeatStep::InvokeModel {
                clips: self.ids.clone(),
            }
        } else {
            BeatStep::Halt
        })
    }
}
#[derive(Default)]
struct Provider(Mutex<Vec<Vec<ModelMessage>>>);
impl ModelClient for Provider {
    fn complete(&self, _: &ModelSpec, messages: Vec<ModelMessage>) -> Result<Completion, String> {
        self.0.lock().unwrap().push(messages);
        Ok(Completion {
            content: "done".into(),
            usage: None,
        })
    }
}
fn select(control: &AgentControl, ids: Vec<String>) -> (Arc<Select>, Arc<Provider>) {
    control
        .register(&listed(&[("scene", &["reader"])], &[], &[]))
        .unwrap();
    control
        .push_event("scene", "read selected evidence")
        .unwrap();
    let body = Arc::new(Select {
        ids,
        seen: Mutex::new(vec![]),
    });
    control
        .mount_agent(
            "reader",
            body.clone(),
            &declares_model(&["scene"], &[], crate::act::ModelFacet::Complete),
        )
        .unwrap();
    let provider = Arc::new(Provider::default());
    control.set_model_client(provider.clone());
    (body, provider)
}

#[tokio::test]
async fn missing_or_corrupt_selected_tool_bodies_never_reach_provider() {
    for remember in [false, true] {
        for kind in [Kind::Action, Kind::Observation] {
            for missing in [false, true] {
                let tmp = TempDir::new("selected-tool-payload");
                let control =
                    AgentControl::open(tmp.path(), &[("count", super::supervision::count_ctor)])
                        .unwrap();
                let mut profile = card("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", "count", Permit::Go);
                if remember {
                    profile.tools[0].tag.memory = MemoryFacet::Remember { aspect: None };
                }
                let id = control.open_thread(&profile).unwrap();
                let result = control.run(id, &super::supervision::call(1)).await.unwrap();
                let event = if remember {
                    control
                        .events(id)
                        .unwrap()
                        .into_iter()
                        .find(|e| e.tags.iter().any(|t| t == "memory"))
                        .unwrap()
                } else if kind == Kind::Action {
                    result.action.unwrap()
                } else {
                    result.observation.unwrap()
                };
                let reference: Value = serde_json::from_str(&event.content).unwrap();
                let path = control
                    .directory(id)
                    .unwrap()
                    .join("blobs")
                    .join(reference["sha256"].as_str().unwrap());
                if missing {
                    fs::remove_file(&path).unwrap();
                } else {
                    fs::write(&path, b"wrong").unwrap();
                }
                let (_body, provider) = select(&control, vec![event.id.clone()]);
                assert!(control.offer(id, "reader").is_err());
                assert!(provider.0.lock().unwrap().is_empty());
                assert!(
                    control
                        .events(id)
                        .unwrap()
                        .iter()
                        .any(|e| e.id == event.id && e.content == event.content)
                );
            }
        }
    }
}

struct Echo(&'static str);
#[async_trait]
impl Tool for Echo {
    fn name(&self) -> &str {
        self.0
    }
    fn description(&self) -> &str {
        "identity fixture"
    }
    fn parameters(&self) -> Value {
        json!({})
    }
    async fn execute(&self, args: Value, _: &ToolCtx) -> Result<String, String> {
        Ok(args.to_string())
    }
}
fn left() -> Arc<dyn Tool> {
    Arc::new(Echo("left"))
}
fn right() -> Arc<dyn Tool> {
    Arc::new(Echo("right"))
}

#[tokio::test]
async fn same_arguments_keep_distinct_tools_in_projection_and_model_context() {
    let tmp = TempDir::new("tool-context-identity");
    let control = AgentControl::open(tmp.path(), &[("left", left), ("right", right)]).unwrap();
    let mut profile = card("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", "left", Permit::Go);
    let mut second = profile.tools[0].clone();
    second.name = "right".into();
    profile.tools.push(second);
    let id = control.open_thread(&profile).unwrap();
    let mut ids = vec![];
    for name in ["left", "right"] {
        let result = control
            .run(
                id,
                &ToolCall {
                    id: name.into(),
                    name: name.into(),
                    arguments: "{}".into(),
                },
            )
            .await
            .unwrap();
        ids.push(result.action.unwrap().id);
    }
    let (body, provider) = select(&control, ids);
    control.offer(id, "reader").unwrap();
    let seen = body.seen.lock().unwrap();
    let projection = seen[0].to_string();
    let calls = provider.0.lock().unwrap();
    assert_eq!(calls.len(), 1);
    let messages = calls[0]
        .iter()
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    for name in ["left", "right"] {
        assert!(
            projection
                .contains(&serde_json::to_string(&format!("[tool \"{name}\"]\n{{}}")).unwrap())
        );
        assert!(messages.contains(&format!("[tool \"{name}\"]\n{{}}")));
    }
}

#[test]
fn selected_model_payloads_also_fail_closed() {
    use crate::memstream::{AppendEvent, FromKind};
    use sha2::{Digest, Sha256};
    for missing in [false, true] {
        let tmp = TempDir::new("selected-model-payload");
        let control =
            AgentControl::open(tmp.path(), &[("count", super::supervision::count_ctor)]).unwrap();
        let id = control
            .open_thread(&card(
                "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
                "count",
                Permit::Go,
            ))
            .unwrap();
        let sha = format!("{:x}", Sha256::digest(b"answer"));
        let agent = control.live(id).unwrap();
        let event = agent
            .session()
            .tape()
            .append(AppendEvent {
                from: "model".into(),
                from_kind: FromKind::Agent,
                kind: Kind::Observation,
                session: "s".into(),
                content: json!({"response_sha256": sha, "response_bytes": 6}).to_string(),
                tags: vec![],
                refs: vec![],
                act: None,
                card: None,
            })
            .unwrap();
        let dir = control.directory(id).unwrap().join("blobs");
        fs::create_dir_all(&dir).unwrap();
        if !missing {
            fs::write(dir.join(&sha), b"broken").unwrap();
        }
        let (_, provider) = select(&control, vec![event.id]);
        assert!(control.offer(id, "reader").is_err());
        assert!(provider.0.lock().unwrap().is_empty());
    }
}
