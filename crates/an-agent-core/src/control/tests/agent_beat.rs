//! Agent bodies on the delivery path: core drives the body's steps, and
//! every model call is admitted against the card and taped as its own
//! anchored action + observation pair — never hidden inside one note.

use std::sync::Mutex;

use super::*;
use super::{Ink, declares, listed, stroke_card};

/// A body that thinks: first asks for a model call over the newest clip,
/// then utters what the model observed back.
struct Stage {
    seen: Mutex<Vec<Value>>,
}

impl AgentBody for Stage {
    fn evaluate(&self, projection: &Value) -> Result<BeatStep, String> {
        self.seen
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(projection.clone());
        let steps = projection["steps"].as_u64().unwrap_or(0);
        let clips = projection["clips"].as_array().cloned().unwrap_or_default();
        let last = clips.last().cloned().unwrap_or_else(|| json!({}));
        let clip = last["id"].as_str().unwrap_or_default().to_string();
        match steps {
            0 => Ok(BeatStep::InvokeModel { clips: vec![clip] }),
            _ => Ok(BeatStep::Utter {
                text: format!("npc: {}", last["content"].as_str().unwrap_or_default()),
            }),
        }
    }
}

struct Silent;

impl AgentBody for Silent {
    fn evaluate(&self, _projection: &Value) -> Result<BeatStep, String> {
        Ok(BeatStep::Halt)
    }
}

/// Never concludes: asks for a model call every step.
struct Loopy(String);

impl AgentBody for Loopy {
    fn evaluate(&self, _projection: &Value) -> Result<BeatStep, String> {
        Ok(BeatStep::InvokeModel {
            clips: vec![self.0.clone()],
        })
    }
}

struct Rude;

impl AgentBody for Rude {
    fn evaluate(&self, _projection: &Value) -> Result<BeatStep, String> {
        Err("forgot lines".into())
    }
}

struct Handy;

impl AgentBody for Handy {
    fn evaluate(&self, _projection: &Value) -> Result<BeatStep, String> {
        Ok(BeatStep::InvokeTool {
            name: "bash".into(),
            args: json!({}),
        })
    }
}

struct StubModel {
    calls: Mutex<Vec<(String, Vec<ModelMessage>)>>,
    fail: bool,
}

impl ModelClient for StubModel {
    fn complete(
        &self,
        spec: &ModelSpec,
        messages: Vec<ModelMessage>,
    ) -> Result<Completion, String> {
        self.calls
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push((spec.model.clone(), messages));
        if self.fail {
            Err("model offline".into())
        } else {
            Ok(Completion {
                content: "canned reply".into(),
                usage: Some(Usage {
                    input_tokens: 42,
                    output_tokens: 7,
                }),
            })
        }
    }
}

fn stub_model() -> Arc<StubModel> {
    Arc::new(StubModel {
        calls: Mutex::new(Vec::new()),
        fail: false,
    })
}

/// The persona beat: scene noted on the tape, the body asks for one model
/// call over it, and the answer comes back as the reply. Both halves of
/// the model call are taped with their anchor refs. The envelope the
/// client receives is the thread's composition: card prompt, host facts,
/// workspace env, and declared guidance.
#[test]
fn an_agent_body_thinks_and_every_step_is_taped() {
    let tmp = TempDir::new("control-agent-beat");
    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    control
        .register(&listed(&[("scene", &["npc"])], &["main"], &["main"]))
        .unwrap();
    control
        .put_env("main", "# The Tavern\nA quiet place.")
        .unwrap();
    control
        .put_config(
            "main",
            br#"
[guidance]
rules = ["stay in character", "replies under 280 characters"]
"#,
        )
        .unwrap();
    let id = control.open_thread(&stroke_card()).unwrap();
    control.push_event("scene", "a traveler enters").unwrap();
    let scene = control.note(id, "scene", "a traveler enters").unwrap();

    let model = stub_model();
    control.set_model_client(model.clone());
    control.set_host_label("test-app");
    let stage = Arc::new(Stage {
        seen: Mutex::new(Vec::new()),
    });
    control
        .mount_agent(
            "npc",
            stage.clone(),
            &declares_model(&["scene"], &[], crate::act::ModelFacet::Complete),
        )
        .unwrap();

    let reply = said(control.offer(id, "npc").unwrap());
    assert_eq!(reply.spool, "npc");
    assert!(reply.reply.contains("canned reply"));
    assert_eq!(reply.intents.len(), 1);

    // The client saw the card's model and the composed envelope: prompt,
    // host facts, workspace env, declared guidance, plus the cited clip.
    let calls = model.calls.lock().unwrap_or_else(|err| err.into_inner());
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "m");
    let system = &calls[0].1[0];
    assert_eq!(system.role, "system");
    for needle in [
        "p\n\n# Host",
        "test-app on ",
        "# Environment\n# The Tavern\nA quiet place.",
        "# Guidance (advisory)\n- stay in character\n- replies under 280 characters",
    ] {
        assert!(system.content.contains(needle), "{needle}");
    }
    assert!(calls[0].1[1].content.contains("a traveler enters"));
    drop(calls);

    // The model call is two anchored events: the action cites the clip it
    // read, the observation cites the action. The observation is thin —
    // hash, size, usage — and the payload lives in the session's blob
    // store. The spool note carries the reply.
    let tape = control.events(id).unwrap();
    let action = tape
        .iter()
        .find(|event| event.kind == Kind::Action && event.tags.iter().any(|tag| tag == "invoke"))
        .expect("invoke action");
    assert!(action.refs.iter().any(|r| r == &scene));
    assert!(action.content.contains("\"spool\":\"npc\""));
    let observation = tape
        .iter()
        .find(|event| {
            event.kind == Kind::Observation && event.tags.iter().any(|tag| tag == "invoke")
        })
        .expect("invoke observation");
    assert!(observation.refs.iter().any(|r| r == &action.id));
    assert!(!observation.content.contains("canned reply"));
    let note: Value = serde_json::from_str(&observation.content).unwrap();
    assert_eq!(note["usage"]["input_tokens"], 42);
    assert_eq!(note["usage"]["output_tokens"], 7);
    let sha = note["response_sha256"].as_str().expect("response sha");
    let blob = std::fs::read(control.directory(id).unwrap().join("blobs").join(sha)).unwrap();
    assert_eq!(blob, b"canned reply");
    assert!(tape.iter().any(|event| {
        event.tags.iter().any(|tag| tag == "spool")
            && event.content.contains("\"spool\":\"npc\"")
            && event.content.contains("canned reply")
    }));

    // The body saw the model's answer join the projection on step 1 —
    // resolved back from the blob, so the tape stays thin.
    let seen = stage.seen.lock().unwrap_or_else(|err| err.into_inner());
    assert_eq!(seen.len(), 2);
    let step1_clips = seen[1]["clips"].as_array().expect("clips");
    assert!(
        step1_clips
            .last()
            .and_then(|clip| clip["content"].as_str())
            .is_some_and(|content| content.contains("canned reply"))
    );
}

/// A halt is a taped, explicit silence: it comes back as
/// `DeliveryOutcome::Halt` carrying its own note id — never a
/// synthesized empty reply, and it earns no `applied`.
#[test]
fn a_halt_is_taped_and_settled_as_silence() {
    let tmp = TempDir::new("control-agent-halt");
    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    control
        .register(&listed(&[("scene", &["npc", "ink"])], &[], &[]))
        .unwrap();
    let id = control.open_thread(&stroke_card()).unwrap();
    control.push_event("scene", "nothing happens").unwrap();
    control
        .mount_agent("npc", Arc::new(Silent), &declares(&["scene"], &[]))
        .unwrap();
    control
        .mount_spool(
            "ink",
            Arc::new(Ink {
                seen: Mutex::new(None),
            }),
            &declares(&["scene"], &[]),
        )
        .unwrap();

    let outcomes = control.dispatch(id).unwrap();
    assert_eq!(outcomes.len(), 2);
    let DeliveryOutcome::Halt { spool, tape_id, .. } = &outcomes[0] else {
        panic!("npc halted, got {:?}", outcomes[0])
    };
    assert_eq!(spool, "npc");
    // The silence note id is real: it is on the tape.
    let tape = control.events(id).unwrap();
    assert!(tape.iter().any(|event| &event.id == tape_id
        && event.tags.iter().any(|tag| tag == "spool")
        && event.content.contains("\"reply\":\"\"")));
    let ink = said(outcomes[1].clone());
    assert_eq!(ink.spool, "ink");

    control.push_event("scene", "still nothing").unwrap();
    let outcome = control.offer(id, "npc").unwrap();
    assert!(matches!(outcome, DeliveryOutcome::Halt { .. }));
}

/// A body that never concludes burns at most MAX_STEPS model calls, then
/// fails like any failed spool: noted and unmounted.
#[test]
fn a_runaway_body_is_stopped_and_unmounted() {
    let tmp = TempDir::new("control-agent-runaway");
    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    control
        .register(&listed(&[("scene", &["npc"])], &[], &[]))
        .unwrap();
    let id = control.open_thread(&stroke_card()).unwrap();
    control.push_event("scene", "spin").unwrap();
    let scene = control.note(id, "scene", "spin").unwrap();
    let model = stub_model();
    control.set_model_client(model.clone());
    control
        .mount_agent(
            "npc",
            Arc::new(Loopy(scene)),
            &declares_model(&["scene"], &[], crate::act::ModelFacet::Complete),
        )
        .unwrap();

    assert!(matches!(
        control.offer(id, "npc"),
        Err(ControlError::SpoolFailed { reason, .. }) if reason.contains("did not conclude")
    ));
    assert_eq!(
        model
            .calls
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .len(),
        8
    );
    assert!(matches!(
        control.offer(id, "npc"),
        Err(ControlError::NotMounted(_))
    ));
}

/// Body errors fail like spool failures. Infra failures — no client, an
/// unknown clip, a model error — propagate and leave the body mounted.
#[test]
fn body_errors_unmount_but_infra_errors_do_not() {
    let tmp = TempDir::new("control-agent-errors");
    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    control
        .register(&listed(&[("scene", &["npc"])], &[], &[]))
        .unwrap();
    let id = control.open_thread(&stroke_card()).unwrap();
    control.push_event("scene", "enter").unwrap();
    let scene = control.note(id, "scene", "enter").unwrap();

    // No client installed: the body asked for a model call and failed closed.
    control
        .mount_agent(
            "npc",
            Arc::new(Loopy(scene.clone())),
            &declares_model(&["scene"], &[], crate::act::ModelFacet::Complete),
        )
        .unwrap();
    assert!(matches!(
        control.offer(id, "npc"),
        Err(ControlError::ModelClientMissing)
    ));
    // Still mounted: install the client and the same body runs.
    control.set_model_client(stub_model());
    control.unmount_spool("npc").unwrap();

    // An unknown clip is a driver error, not a body fault.
    control
        .mount_agent(
            "npc",
            Arc::new(Loopy("no-such-clip".into())),
            &declares_model(&["scene"], &[], crate::act::ModelFacet::Complete),
        )
        .unwrap();
    assert!(matches!(
        control.offer(id, "npc"),
        Err(ControlError::UnknownClip(clip)) if clip == "no-such-clip"
    ));
    control.unmount_spool("npc").unwrap();

    // A failing model is a model error; the body stays mounted.
    control.set_model_client(Arc::new(StubModel {
        calls: Mutex::new(Vec::new()),
        fail: true,
    }));
    control
        .mount_agent(
            "npc",
            Arc::new(Loopy(scene)),
            &declares_model(&["scene"], &[], crate::act::ModelFacet::Complete),
        )
        .unwrap();
    assert!(matches!(
        control.offer(id, "npc"),
        Err(ControlError::ModelFailed { spool, reason }) if spool == "npc" && reason == "model offline"
    ));
    // The failed call is closed on tape: a terminal observation citing
    // its action, so no invoke dangles unreadable as "still in flight".
    let tape = control.events(id).unwrap();
    let action = tape
        .iter()
        .find(|event| event.kind == Kind::Action && event.tags.iter().any(|tag| tag == "invoke"))
        .expect("invoke action");
    assert!(tape.iter().any(|event| {
        event.kind == Kind::Observation
            && event.tags.iter().any(|tag| tag == "invoke")
            && event.refs.iter().any(|r| r == &action.id)
            && event.content.contains("\"status\":\"failed\"")
            && event.content.contains("model offline")
    }));
    control.unmount_spool("npc").unwrap();

    // A body error unmounts, like any failed spool.
    control
        .mount_agent("npc", Arc::new(Rude), &declares(&["scene"], &[]))
        .unwrap();
    assert!(matches!(
        control.offer(id, "npc"),
        Err(ControlError::SpoolFailed { reason, .. }) if reason == "forgot lines"
    ));
    assert!(matches!(
        control.offer(id, "npc"),
        Err(ControlError::NotMounted(_))
    ));
    assert!(control.events(id).unwrap().iter().any(|event| {
        event.tags.iter().any(|tag| tag == "spool") && event.content.contains("forgot lines")
    }));
}

/// `invoke_tool` is not admitted on the delivery path: the driver is sync
/// and tool acts are async. The refusal fails the body like any error.
#[test]
fn invoke_tool_is_refused_in_agent_beats() {
    let tmp = TempDir::new("control-agent-tool");
    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    control
        .register(&listed(&[("scene", &["npc"])], &[], &[]))
        .unwrap();
    let id = control.open_thread(&stroke_card()).unwrap();
    control.push_event("scene", "enter").unwrap();
    control
        .mount_agent("npc", Arc::new(Handy), &declares(&["scene"], &[]))
        .unwrap();
    assert!(matches!(
        control.offer(id, "npc"),
        Err(ControlError::SpoolFailed { reason, .. }) if reason.contains("not admitted")
    ));
    assert!(matches!(
        control.offer(id, "npc"),
        Err(ControlError::NotMounted(_))
    ));
}

/// The model face is default-deny: a body mounted without it that asks for
/// a model call is refused and unmounted like any failed body. This is the
/// mount-time claim enforced on the delivery path — the declaration is the
/// ceiling, not a hint.
#[test]
fn invoke_model_requires_the_mounted_model_face() {
    let tmp = TempDir::new("control-agent-model-face");
    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    control
        .register(&listed(&[("scene", &["npc"])], &[], &[]))
        .unwrap();
    let id = control.open_thread(&stroke_card()).unwrap();
    control.push_event("scene", "enter").unwrap();
    let scene = control.note(id, "scene", "enter").unwrap();
    let model = stub_model();
    control.set_model_client(model.clone());
    control
        .mount_agent("npc", Arc::new(Loopy(scene)), &declares(&["scene"], &[]))
        .unwrap();
    assert!(matches!(
        control.offer(id, "npc"),
        Err(ControlError::SpoolFailed { reason, .. }) if reason.contains("model face")
    ));
    // Refused before any call left the house.
    assert!(
        model
            .calls
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .is_empty()
    );
    assert!(matches!(
        control.offer(id, "npc"),
        Err(ControlError::NotMounted(_))
    ));
}

/// An agent reply goes through the same intent admission as a beat reply:
/// a declared raise travels, an undeclared one is stripped and taped.
#[test]
fn agent_replies_pass_intent_admission() {
    let tmp = TempDir::new("control-agent-intents");
    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    control
        .register(&listed(&[("scene", &["npc"]), ("mood", &[])], &[], &[]))
        .unwrap();
    let id = control.open_thread(&stroke_card()).unwrap();
    control.push_event("scene", "enter").unwrap();

    struct Bard;
    impl AgentBody for Bard {
        fn evaluate(&self, _projection: &Value) -> Result<BeatStep, String> {
            Ok(BeatStep::Utter {
                text: r#"{"utter": "hello", "raise": [{"event": "mood", "body": "warm"}, {"event": "weather", "body": "rain"}]}"#
                    .into(),
            })
        }
    }
    control
        .mount_agent("npc", Arc::new(Bard), &declares(&["scene"], &["mood"]))
        .unwrap();

    let reply = said(control.offer(id, "npc").unwrap());
    assert_eq!(reply.intents.len(), 2);
    assert!(matches!(
        &reply.intents[0],
        ReplyIntent::Utter { text } if text == "hello"
    ));
    assert!(matches!(
        &reply.intents[1],
        ReplyIntent::Raise { event, body } if event == "mood" && body == "warm"
    ));
    let tape = control.events(id).unwrap();
    assert!(tape.iter().any(|event| {
        event.tags.iter().any(|tag| tag == "intent")
            && event.content.contains("weather")
            && event.content.contains("deny")
    }));
}

/// Byte fuses cap what crosses a boundary in one step: a giant clip
/// reaches the body and the model cut and marked, a giant env enters the
/// envelope cut and marked with the full text's hash, and a giant
/// utterance is cut once, before both the tape and the host see it.
#[test]
fn byte_fuses_cap_what_crosses_a_boundary() {
    let tmp = TempDir::new("control-agent-fuses");
    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    control
        .register(&listed(&[("scene", &["npc"])], &[], &["main"]))
        .unwrap();
    let id = control.open_thread(&stroke_card()).unwrap();
    let big_clip = "x".repeat(9_000);
    control.push_event("scene", "loud scene").unwrap();
    control.note(id, "scene", &big_clip).unwrap();
    control.put_env("main", &"e".repeat(20_000)).unwrap();

    struct Loud {
        seen: Mutex<Vec<Value>>,
    }
    impl AgentBody for Loud {
        fn evaluate(&self, projection: &Value) -> Result<BeatStep, String> {
            self.seen
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .push(projection.clone());
            if projection["steps"].as_u64().unwrap_or(0) == 0 {
                let clips = projection["clips"].as_array().cloned().unwrap_or_default();
                let last = clips.last().cloned().unwrap_or_else(|| json!({}));
                Ok(BeatStep::InvokeModel {
                    clips: vec![last["id"].as_str().unwrap_or_default().to_string()],
                })
            } else {
                Ok(BeatStep::Utter {
                    text: "u".repeat(20_000),
                })
            }
        }
    }
    let model = stub_model();
    control.set_model_client(model.clone());
    let loud = Arc::new(Loud {
        seen: Mutex::new(Vec::new()),
    });
    control
        .mount_agent(
            "npc",
            loud.clone(),
            &declares_model(&["scene"], &[], crate::act::ModelFacet::Complete),
        )
        .unwrap();

    let reply = said(control.offer(id, "npc").unwrap());

    // The clip the body saw is cut and marked; the id still names the
    // full original on tape.
    let seen = loud.seen.lock().unwrap_or_else(|err| err.into_inner());
    let clip = seen[0]["clips"]
        .as_array()
        .expect("clips")
        .last()
        .cloned()
        .expect("a clip");
    let content = clip["content"].as_str().expect("content");
    assert!(content.contains("[clip truncated, 9000 bytes total]"));
    assert!(content.len() < 9_000);

    // The model envelope carries both cuts: the clip marked with its
    // size, the env marked with the full text's hash.
    let calls = model.calls.lock().unwrap_or_else(|err| err.into_inner());
    let system = &calls[0].1[0].content;
    assert!(system.contains("[env truncated, full text sha256:"));
    let user = &calls[0].1[1].content;
    assert!(user.contains("[clip truncated, 9000 bytes total]"));
    drop(calls);

    // The utterance is cut once: tape and host carry the same marked text.
    assert!(
        reply
            .reply
            .contains("[utterance truncated, 20000 bytes total]")
    );
    assert!(reply.reply.len() < 20_000);
    let tape = control.events(id).unwrap();
    assert!(tape.iter().any(|event| {
        event.tags.iter().any(|tag| tag == "spool")
            && event.content.contains("utterance truncated")
            && event.content.len() < 20_000
    }));
}

/// A declared guard with no hook runner fails closed at the reply: the
/// envelope may call a rule "enforced" only while the enforcement exists.
#[test]
fn a_declared_guard_without_a_runner_fails_closed() {
    let tmp = TempDir::new("control-agent-guard-no-runner");
    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    control
        .register(&listed(&[("scene", &["npc"])], &["main"], &[]))
        .unwrap();
    control
        .put_config(
            "main",
            br#"[[guard]]
rule = "replies under 280 characters"
gate = { type = "spool", name = "length", version = "1.0.0" }
"#,
        )
        .unwrap();
    let id = control.open_thread(&stroke_card()).unwrap();
    control.push_event("scene", "enter").unwrap();

    struct Bard;
    impl AgentBody for Bard {
        fn evaluate(&self, _projection: &Value) -> Result<BeatStep, String> {
            Ok(BeatStep::Utter {
                text: "hello".into(),
            })
        }
    }
    control
        .mount_agent("npc", Arc::new(Bard), &declares(&["scene"], &[]))
        .unwrap();
    assert!(matches!(
        control.offer(id, "npc"),
        Err(ControlError::HookRunnerMissing)
    ));
}
