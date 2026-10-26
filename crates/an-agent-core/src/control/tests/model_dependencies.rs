//! Real delivery/model boundaries, with mutations made while a provider call
//! is in flight. No duplicate admission path or simulated ledger is used.

use std::sync::atomic::{AtomicUsize, Ordering};

use an_agent_semantic::{
    Dependency, DependencyAssessment, DependencyObservation, Outcome, Receipt, Validity,
    assess_dependencies,
};

use super::*;

struct Respond;

impl AgentBody for Respond {
    fn evaluate(&self, projection: &Value) -> Result<BeatStep, String> {
        if projection["steps"] == 0 {
            Ok(BeatStep::InvokeModel { clips: vec![] })
        } else {
            let clips = projection["clips"].as_array().unwrap();
            Ok(BeatStep::Utter {
                text: clips.last().unwrap()["content"].as_str().unwrap().into(),
            })
        }
    }
}

struct CallbackModel {
    callback: Box<dyn Fn() + Send + Sync>,
    calls: Arc<AtomicUsize>,
    fail: bool,
}

impl ModelClient for CallbackModel {
    fn complete(&self, _: &ModelSpec, _: Vec<ModelMessage>) -> Result<Completion, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        (self.callback)();
        if self.fail {
            Err("provider outcome uncertain".into())
        } else {
            Ok(Completion {
                content: "retained model payload".into(),
                usage: None,
            })
        }
    }
}

fn install(
    control: &AgentControl,
    callback: impl Fn() + Send + Sync + 'static,
    fail: bool,
) -> Arc<AtomicUsize> {
    let calls = Arc::new(AtomicUsize::new(0));
    control.set_model_client(Arc::new(CallbackModel {
        callback: Box::new(callback),
        calls: calls.clone(),
        fail,
    }));
    calls
}

fn registration() -> Registration {
    listed(&[("scene", &["npc"])], &["main"], &["main"])
}

fn declaration() -> SpoolDeclaration {
    declares_model(&["scene"], &[], crate::act::ModelFacet::Complete)
}

fn setup() -> (TempDir, AgentControl, Uuid, Uuid) {
    let tmp = TempDir::new("model-dependency-boundary");
    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    control.register(&registration()).unwrap();
    control
        .put_config("main", b"[guidance]\nrules = ['original']")
        .unwrap();
    control.put_env("main", "original").unwrap();
    let child = control.open_thread(&stroke_card()).unwrap();
    let main = control
        .open_thread(&card(
            "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
            "bash",
            Permit::Go,
        ))
        .unwrap();
    control.set_supervisor(child, main).unwrap();
    control.push_event("scene", "draw").unwrap();
    control
        .mount_agent("npc", Arc::new(Respond), &declaration())
        .unwrap();
    (tmp, control, child, main)
}

fn review(outcome: DeliveryOutcome) -> ReviewRequest {
    match outcome {
        DeliveryOutcome::NeedsReview(request) => *request,
        other => panic!("expected dependency review, got {other:?}"),
    }
}

fn quarantined(control: &AgentControl, child: Uuid) -> (String, Value) {
    let event = control
        .events(child)
        .unwrap()
        .into_iter()
        .find(|e| e.tags.iter().any(|t| t == "model_quarantined"))
        .unwrap();
    assert!(event.tags.iter().any(|t| t == "audit"));
    let record: Value = serde_json::from_str(&event.content).unwrap();
    let dependencies: Vec<Dependency> =
        serde_json::from_value(record["dependencies"].clone()).unwrap();
    let observations: Vec<DependencyObservation> =
        serde_json::from_value(record["dependency_observations"].clone()).unwrap();
    let assessment: DependencyAssessment =
        serde_json::from_value(record["dependency_assessment"].clone()).unwrap();
    assert_eq!(
        assessment,
        assess_dependencies(&dependencies, &observations)
    );
    assert_eq!(assessment.validity, Validity::Stale);
    (event.id, record)
}

#[test]
fn changed_inputs_quarantine_success_and_request_fresh_work_not_approval() {
    for changed in [
        "config",
        "env",
        "registration",
        "restored_config",
        "restored_env",
        "restored_registration",
    ] {
        let (_tmp, control, child, main) = setup();
        let mutator = control.clone();
        let calls = install(
            &control,
            move || {
                if changed.ends_with("config") {
                    mutator
                        .put_config("main", b"[guidance]\nrules = ['changed']")
                        .unwrap();
                    if changed.starts_with("restored") {
                        mutator
                            .put_config("main", b"[guidance]\nrules = ['original']")
                            .unwrap();
                    }
                } else if changed.ends_with("env") {
                    mutator.put_env("main", "changed").unwrap();
                    if changed.starts_with("restored") {
                        mutator.put_env("main", "original").unwrap();
                    }
                } else {
                    let mut next = registration();
                    next.events.push(EventRoute {
                        name: "extra".into(),
                        spools: vec![],
                    });
                    mutator.register(&next).unwrap();
                    if changed.starts_with("restored") {
                        mutator.register(&registration()).unwrap();
                    }
                }
            },
            false,
        );
        let request = review(control.offer(child, "npc").unwrap());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let (evidence, record) = quarantined(&control, child);
        assert_eq!(
            request.cause,
            ReviewCause::DependencyChanged {
                evidence: evidence.clone()
            }
        );
        assert!(request.attempt.refs.contains(&evidence));
        assert_eq!(record["phase"], "after_call");
        let receipt: Receipt = serde_json::from_value(record["receipt"].clone()).unwrap();
        assert_eq!(receipt.outcome, Outcome::Succeeded);
        let agent = control.live(child).unwrap();
        assert_eq!(
            agent
                .session()
                .read_blob(record["response_sha256"].as_str().unwrap())
                .unwrap(),
            b"retained model payload"
        );
        assert_eq!(
            control.pending_reviews(main).unwrap(),
            vec![request.clone()]
        );
        assert!(
            control
                .events(main)
                .unwrap()
                .iter()
                .any(|e| e.content.contains(&request.id))
        );
        assert!(
            !control.events(child).unwrap().iter().any(|e| e
                .tags
                .iter()
                .any(|t| t == "applied" || t == "spool")
                || (e.kind == Kind::Observation && e.tags.iter().any(|t| t == "invoke")))
        );
        assert!(matches!(
            control.resolve_review(
                main,
                &request.id,
                ReviewResolution::ApproveOnce {
                    note: "use old output".into()
                }
            ),
            Err(ControlError::ReviewCeiling(_))
        ));
        assert_eq!(review(control.offer(child, "npc").unwrap()).id, request.id);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        control
            .resolve_review(
                main,
                &request.id,
                ReviewResolution::Retry {
                    note: "recompute from current inputs".into(),
                },
            )
            .unwrap();
        let fresh_calls = install(&control, || {}, false);
        assert_eq!(
            said(control.offer(child, "npc").unwrap()).reply,
            "retained model payload"
        );
        assert_eq!(fresh_calls.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn unchanged_generations_and_unrelated_mounts_do_not_invalidate_results() {
    let (_tmp, control, child, _) = setup();
    let mutator = control.clone();
    let calls = install(
        &control,
        move || {
            mutator.register(&registration()).unwrap();
            mutator
                .put_config("main", b"[guidance]\nrules = ['original']")
                .unwrap();
            mutator.put_env("main", "original").unwrap();
            mutator
                .mount_agent(
                    "other",
                    Arc::new(Respond),
                    &declares_model(&[], &[], crate::act::ModelFacet::Complete),
                )
                .unwrap();
            mutator.unmount_spool("other").unwrap();
        },
        false,
    );
    assert_eq!(
        said(control.offer(child, "npc").unwrap()).reply,
        "retained model payload"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(
        !control
            .events(child)
            .unwrap()
            .iter()
            .any(|e| e.tags.iter().any(|t| t == "model_quarantined"))
    );
}

#[test]
fn retired_scope_quarantines_without_attaching_a_request_to_its_replacement() {
    for change in ["remount", "revoke_child", "revoke_parent", "cancel"] {
        let (_tmp, control, child, main) = setup();
        let mutator = control.clone();
        install(
            &control,
            move || match change {
                "remount" => {
                    mutator.unmount_spool("npc").unwrap();
                    mutator
                        .mount_agent("npc", Arc::new(Respond), &declaration())
                        .unwrap();
                }
                "revoke_child" => mutator.revoke_reviews(child, "changed authority").unwrap(),
                "revoke_parent" => mutator.revoke_reviews(main, "changed authority").unwrap(),
                _ => {
                    mutator.cancel(child).unwrap();
                }
            },
            false,
        );
        let Err(ControlError::DependencyStale(evidence)) = control.offer(child, "npc") else {
            panic!("expected retired scope");
        };
        assert_eq!(quarantined(&control, child).0, evidence);
        assert!(control.pending_reviews(main).unwrap().is_empty());
        install(&control, || {}, false);
        if change == "cancel" {
            assert!(matches!(
                control.offer(child, "npc"),
                Err(ControlError::Cancelled(_))
            ));
        }
        assert_eq!(
            said(control.offer(child, "npc").unwrap()).reply,
            "retained model payload"
        );
    }
}

#[test]
fn failed_external_call_keeps_unknown_outcome_even_when_quarantined() {
    let (_tmp, control, child, _) = setup();
    let mutator = control.clone();
    install(
        &control,
        move || {
            mutator.put_env("main", "changed").unwrap();
        },
        true,
    );
    review(control.offer(child, "npc").unwrap());
    let (_, record) = quarantined(&control, child);
    let receipt: Receipt = serde_json::from_value(record["receipt"].clone()).unwrap();
    assert_eq!(receipt.outcome, Outcome::Unknown);
    assert_eq!(record["status"], "failed");
    assert!(record.get("response_sha256").is_none());
}

#[test]
fn audit_failure_never_publishes_stale_payload_or_retries_automatically() {
    let (_tmp, control, child, main) = setup();
    let mutator = control.clone();
    let calls = install(
        &control,
        move || {
            mutator.put_env("main", "changed").unwrap();
            mutator
                .live(child)
                .unwrap()
                .session()
                .tape()
                .fail_next_append();
        },
        false,
    );
    assert!(matches!(
        control.offer(child, "npc"),
        Err(ControlError::Store(_))
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(control.pending_reviews(main).unwrap().is_empty());
    assert!(
        !control
            .events(child)
            .unwrap()
            .iter()
            .any(|e| e.kind == Kind::Observation && e.tags.iter().any(|t| t == "invoke"))
    );
}

struct SelectClip(String);

impl AgentBody for SelectClip {
    fn evaluate(&self, projection: &Value) -> Result<BeatStep, String> {
        assert!(
            !projection["clips"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["id"] == self.0)
        );
        Ok(BeatStep::InvokeModel {
            clips: vec![self.0.clone()],
        })
    }
}

#[test]
fn quarantined_payload_is_absent_from_projection_and_explicit_model_clips() {
    let (_tmp, control, child, main) = setup();
    let mutator = control.clone();
    install(
        &control,
        move || {
            mutator.put_env("main", "changed").unwrap();
        },
        false,
    );
    let request = review(control.offer(child, "npc").unwrap());
    let (evidence, _) = quarantined(&control, child);
    control
        .resolve_review(
            main,
            &request.id,
            ReviewResolution::Reject {
                note: "inspect only".into(),
            },
        )
        .unwrap();
    control.unmount_spool("npc").unwrap();
    control
        .mount_agent(
            "npc",
            Arc::new(SelectClip(evidence.clone())),
            &declaration(),
        )
        .unwrap();
    let calls = install(&control, || {}, false);
    assert!(
        matches!(control.offer(child, "npc"), Err(ControlError::IneligibleClip(id)) if id == evidence)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

struct ChangeRegistration(AgentControl);

impl AgentBody for ChangeRegistration {
    fn evaluate(&self, _: &Value) -> Result<BeatStep, String> {
        let mut next = registration();
        next.events.push(EventRoute {
            name: "extra".into(),
            spools: vec![],
        });
        self.0.register(&next).unwrap();
        self.0.register(&registration()).unwrap();
        Ok(BeatStep::InvokeModel { clips: vec![] })
    }
}

#[test]
fn stale_delivery_is_rejected_before_any_external_model_call() {
    let (_tmp, control, child, _) = setup();
    control.unmount_spool("npc").unwrap();
    control
        .mount_agent(
            "npc",
            Arc::new(ChangeRegistration(control.clone())),
            &declaration(),
        )
        .unwrap();
    let calls = install(&control, || {}, false);
    review(control.offer(child, "npc").unwrap());
    let (_, record) = quarantined(&control, child);
    assert_eq!(record["phase"], "before_call");
    assert!(record.get("receipt").is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn corrupt_environment_refuses_before_any_model_call() {
    let (tmp, control, child, _) = setup();
    let cite = control.workspace_cite().unwrap();
    let blob = tmp
        .path()
        .join("workspace/env")
        .join(cite.env_sha256.unwrap());
    std::fs::write(&blob, b"uncommitted bytes").unwrap();
    let calls = install(&control, || {}, false);
    assert!(control.offer(child, "npc").is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(!control.events(child).unwrap().iter().any(|event| {
        event
            .tags
            .iter()
            .any(|tag| tag == "applied" || tag == "model_completion")
    }));
}
