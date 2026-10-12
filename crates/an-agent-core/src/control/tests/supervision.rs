//! Real control paths: no effect before review, trusted recipient, one-shot
//! approval, fresh inputs, and an independently progressing supervisor.

use super::*;
use crate::act::{ActCtx, run_tool_act};

struct Count;

#[async_trait]
impl Tool for Count {
    fn name(&self) -> &str {
        "count"
    }
    fn description(&self) -> &str {
        "count one execution"
    }
    fn parameters(&self) -> Value {
        json!({})
    }
    async fn execute(&self, args: Value, _ctx: &ToolCtx) -> Result<String, String> {
        Ok(args.to_string())
    }
}

pub(super) fn count_ctor() -> Arc<dyn Tool> {
    Arc::new(Count)
}

pub(super) fn setup(permit: Permit) -> (TempDir, AgentControl, Uuid, Uuid) {
    let tmp = TempDir::new("supervision");
    let control = AgentControl::open(tmp.path(), &[("count", count_ctor)]).unwrap();
    let child = control
        .open_thread(&card(
            "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            "count",
            permit,
        ))
        .unwrap();
    let main = control
        .open_thread(&card(
            "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
            "count",
            Permit::Go,
        ))
        .unwrap();
    control.set_supervisor(child, main).unwrap();
    (tmp, control, child, main)
}

pub(super) fn call(value: i32) -> ToolCall {
    ToolCall {
        id: "call-1".into(),
        name: "count".into(),
        arguments: json!({"value": value}).to_string(),
    }
}

pub(super) fn requested(error: ControlError) -> ReviewRequest {
    let ControlError::ReviewRequired(request) = error else {
        panic!("review expected: {error}")
    };
    *request
}

#[tokio::test]
async fn ask_stops_mails_main_and_approval_runs_only_once() {
    let (_tmp, control, child, main) = setup(Permit::Ask);
    let request = requested(control.run(child, &call(1)).await.err().unwrap());
    assert_eq!(request.supervisor, Some(main));
    assert!(matches!(request.cause, ReviewCause::HighRisk { .. }));
    assert!(
        !control
            .events(child)
            .unwrap()
            .iter()
            .any(|e| e.act.is_some())
    );
    assert!(
        control
            .events(main)
            .unwrap()
            .iter()
            .any(|e| e.tags.iter().any(|tag| tag == "send") && e.content.contains(&request.id))
    );
    // Waiting frees the beat: main can work and decide; consult neither
    // contacts the third party nor executes the blocked child operation.
    control.advance(main, "consider the request").unwrap();
    control
        .resolve_review(
            main,
            &request.id,
            ReviewResolution::Consult {
                note: "seek specialist advice".into(),
            },
        )
        .unwrap();
    let again = requested(control.run(child, &call(1)).await.err().unwrap());
    assert_eq!(again.id, request.id);
    control
        .resolve_review(
            main,
            &request.id,
            ReviewResolution::ApproveOnce {
                note: "one exact call".into(),
            },
        )
        .unwrap();
    assert!(
        !control
            .events(child)
            .unwrap()
            .iter()
            .any(|e| e.act.is_some())
    );
    let result = control.run(child, &call(1)).await.unwrap();
    assert_eq!(result.message.content, call(1).arguments);
    assert!(control.pending_reviews(main).unwrap().is_empty());
    let second = requested(control.run(child, &call(1)).await.err().unwrap());
    assert_ne!(request.id, second.id);
    assert!(matches!(
        control.resolve_review(
            main,
            &request.id,
            ReviewResolution::ApproveOnce {
                note: "replay".into()
            }
        ),
        Err(ControlError::ReviewMissing(_))
    ));
}

#[tokio::test]
async fn child_or_unrelated_agent_cannot_resolve_or_rebind_pending_review() {
    let (_tmp, control, child, main) = setup(Permit::Ask);
    let request = requested(control.run(child, &call(1)).await.err().unwrap());
    let unrelated = control
        .open_thread(&card(
            "cccccccc-cccc-cccc-cccc-cccccccccccc",
            "count",
            Permit::Go,
        ))
        .unwrap();
    for caller in [child, unrelated] {
        assert!(matches!(
            control.resolve_review(
                caller,
                &request.id,
                ReviewResolution::ApproveOnce {
                    note: "forged".into()
                }
            ),
            Err(ControlError::ReviewAuthority(_))
        ));
    }
    assert!(control.set_supervisor(child, unrelated).is_err());
    assert_eq!(control.pending_reviews(main).unwrap().len(), 1);
}

#[tokio::test]
async fn approve_cannot_override_deny_or_a_missing_tool() {
    let (_tmp, control, child, main) = setup(Permit::Deny);
    for proposed in [
        call(1),
        ToolCall {
            name: "missing".into(),
            ..call(1)
        },
    ] {
        let request = requested(control.run(child, &proposed).await.err().unwrap());
        assert!(matches!(
            request.cause,
            ReviewCause::PermissionDenied { .. }
        ));
        assert!(matches!(
            control.resolve_review(
                main,
                &request.id,
                ReviewResolution::ApproveOnce {
                    note: "override".into()
                }
            ),
            Err(ControlError::ReviewCeiling(_))
        ));
        control
            .resolve_review(
                main,
                &request.id,
                ReviewResolution::Retry {
                    note: "replan under another legitimate authority".into(),
                },
            )
            .unwrap();
        let next = requested(control.run(child, &proposed).await.err().unwrap());
        assert_ne!(request.id, next.id);
        control
            .resolve_review(
                main,
                &next.id,
                ReviewResolution::Reject {
                    note: "no grant".into(),
                },
            )
            .unwrap();
    }
    assert!(
        !control
            .events(child)
            .unwrap()
            .iter()
            .any(|e| e.act.is_some())
    );
}

#[tokio::test]
async fn approval_binds_arguments_and_workspace_revision() {
    let (_tmp, control, child, main) = setup(Permit::Ask);
    control.register(&listed(&[], &["main"], &[])).unwrap();
    control
        .put_config("main", b"[guidance]\nrules = [\"first\"]\n")
        .unwrap();
    let request = requested(control.run(child, &call(1)).await.err().unwrap());
    control
        .resolve_review(
            main,
            &request.id,
            ReviewResolution::ApproveOnce {
                note: "exact inputs".into(),
            },
        )
        .unwrap();
    assert!(matches!(
        control.run(child, &call(2)).await,
        Err(ControlError::ReviewStale(_))
    ));
    assert!(control.pending_reviews(main).unwrap().is_empty());
    let fresh = requested(control.run(child, &call(1)).await.err().unwrap());
    assert_ne!(fresh.id, request.id);
    control
        .resolve_review(
            main,
            &fresh.id,
            ReviewResolution::ApproveOnce {
                note: "fresh inputs".into(),
            },
        )
        .unwrap();
    control
        .put_config("main", b"[guidance]\nrules = [\"second\"]\n")
        .unwrap();
    assert!(control.pending_reviews(main).unwrap().is_empty());
    let changed = requested(control.run(child, &call(1)).await.err().unwrap());
    assert_ne!(changed.id, fresh.id);
    assert!(
        !control
            .events(child)
            .unwrap()
            .iter()
            .any(|e| e.act.is_some())
    );
}

#[tokio::test]
async fn cancel_invalidates_an_unused_approval() {
    let (_tmp, control, child, main) = setup(Permit::Ask);
    let request = requested(control.run(child, &call(1)).await.err().unwrap());
    control
        .resolve_review(
            main,
            &request.id,
            ReviewResolution::ApproveOnce {
                note: "one call".into(),
            },
        )
        .unwrap();
    control.cancel(child).unwrap();
    assert!(control.pending_reviews(main).unwrap().is_empty());
    assert!(matches!(
        control.run(child, &call(1)).await,
        Err(ControlError::Cancelled(_))
    ));
    let next = requested(control.run(child, &call(1)).await.err().unwrap());
    assert_ne!(request.id, next.id);
}

#[tokio::test]
async fn public_tool_wrapper_cannot_treat_ask_as_go() {
    let tmp = TempDir::new("ask-wrapper");
    let specification = card("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", "count", Permit::Ask);
    let built =
        crate::principal::factory::build_with(&specification, "hash", &[("count", count_ctor)])
            .unwrap();
    let store = crate::memstream::JsonlStore::open(tmp.path().join("tape.jsonl")).unwrap();
    let context = ActCtx {
        store: Some(&store),
        agent_id: "child",
        session: "s",
        card: Some("hash"),
    };
    assert!(matches!(
        run_tool_act(&context, &built.tools, &call(1), &ToolCtx { signal: None }).await,
        Err(ToolError::ApprovalRequired(_))
    ));
    assert!(store.read_all().unwrap().is_empty());
}

pub(super) struct Cautious;
impl AgentBody for Cautious {
    fn evaluate(&self, _projection: &Value) -> Result<BeatStep, String> {
        Ok(BeatStep::RequestReview {
            reason: "this change may destroy work".into(),
        })
    }
}

#[test]
fn a_body_can_request_review_without_unmount_or_automatic_restart() {
    let (_tmp, control, child, main) = setup(Permit::Go);
    control
        .register(&listed(&[("scene", &["child-body"])], &[], &[]))
        .unwrap();
    control
        .mount_agent("child-body", Arc::new(Cautious), &declares(&["scene"], &[]))
        .unwrap();
    let event = control.push_event("scene", "change").unwrap();
    let DeliveryOutcome::NeedsReview(request) =
        control.offer_on(child, "child-body", &event.id).unwrap()
    else {
        panic!("review expected")
    };
    assert!(matches!(request.cause, ReviewCause::HighRisk { .. }));
    let count = control.events(child).unwrap().len();
    let DeliveryOutcome::NeedsReview(again) =
        control.offer_on(child, "child-body", &event.id).unwrap()
    else {
        panic!("still waiting")
    };
    assert_eq!(request.id, again.id);
    assert_eq!(control.events(child).unwrap().len(), count);
    control
        .resolve_review(
            main,
            &request.id,
            ReviewResolution::Retry {
                note: "use a non-destructive plan".into(),
            },
        )
        .unwrap();
    assert!(control.pending_reviews(main).unwrap().is_empty());
    // The body remains mounted, but fresh scheduling still requires host
    // initiative; a Retry response does not interpret or bypass the body.
    assert!(matches!(
        control.offer_on(child, "child-body", &event.id).unwrap(),
        DeliveryOutcome::NeedsReview(_)
    ));
}

#[test]
fn supervisor_wiring_rejects_self_and_cycles() {
    let (_tmp, control, child, main) = setup(Permit::Go);
    assert!(control.set_supervisor(main, child).is_err());
    assert!(control.set_supervisor(child, child).is_err());
}

#[tokio::test]
async fn supervisor_needs_its_own_go_authority_to_approve() {
    let tmp = TempDir::new("supervisor-ceiling");
    let control = AgentControl::open(tmp.path(), &[("count", count_ctor)]).unwrap();
    let child = control
        .open_thread(&card(
            "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            "count",
            Permit::Ask,
        ))
        .unwrap();
    let main = control
        .open_thread(&card(
            "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
            "count",
            Permit::Deny,
        ))
        .unwrap();
    control.set_supervisor(child, main).unwrap();
    let request = requested(control.run(child, &call(1)).await.err().unwrap());
    assert!(matches!(
        control.resolve_review(
            main,
            &request.id,
            ReviewResolution::ApproveOnce {
                note: "no authority".into()
            }
        ),
        Err(ControlError::ReviewCeiling(_))
    ));
    assert!(
        !control
            .events(child)
            .unwrap()
            .iter()
            .any(|e| e.act.is_some())
    );
}

#[tokio::test]
async fn supervisor_can_revoke_an_unused_approval() {
    let (_tmp, control, child, main) = setup(Permit::Ask);
    let request = requested(control.run(child, &call(1)).await.err().unwrap());
    control
        .resolve_review(
            main,
            &request.id,
            ReviewResolution::ApproveOnce {
                note: "proceed once".into(),
            },
        )
        .unwrap();
    control
        .resolve_review(
            main,
            &request.id,
            ReviewResolution::Consult {
                note: "wait for an opinion".into(),
            },
        )
        .unwrap();
    assert_eq!(
        requested(control.run(child, &call(1)).await.err().unwrap()).id,
        request.id
    );
    control
        .resolve_review(
            main,
            &request.id,
            ReviewResolution::Reject {
                note: "do not proceed".into(),
            },
        )
        .unwrap();
    assert!(control.pending_reviews(main).unwrap().is_empty());
    assert!(
        !control
            .events(child)
            .unwrap()
            .iter()
            .any(|e| e.act.is_some())
    );
}

#[test]
fn one_denied_delivery_does_not_abort_other_recipients() {
    let (_tmp, control, child, main) = setup(Permit::Go);
    control
        .register(&listed(
            &[("stroke", &["blocked"]), ("other", &["healthy"])],
            &[],
            &[],
        ))
        .unwrap();
    control
        .mount_spool(
            "blocked",
            Arc::new(Ink {
                seen: Mutex::new(None),
            }),
            &declares(&["stroke"], &[]),
        )
        .unwrap();
    control
        .mount_spool(
            "healthy",
            Arc::new(Ink {
                seen: Mutex::new(None),
            }),
            &declares(&["other"], &[]),
        )
        .unwrap();
    control
        .register(&listed(
            &[("stroke", &[]), ("other", &["blocked", "healthy"])],
            &[],
            &[],
        ))
        .unwrap();
    let event = control.push_event("other", "fresh input").unwrap();
    let outcomes = control.dispatch_on(child, &event.id).unwrap();
    assert_eq!(outcomes.len(), 2);
    assert!(matches!(&outcomes[0], DeliveryOutcome::NeedsReview(_)));
    assert_eq!(outcomes[1].reply().unwrap().spool, "healthy");
    assert_eq!(control.pending_reviews(main).unwrap().len(), 1);
}

#[tokio::test]
async fn an_unassigned_request_cannot_be_approved_as_a_serialized_token() {
    let tmp = TempDir::new("unassigned-review");
    let control = AgentControl::open(tmp.path(), &[("count", count_ctor)]).unwrap();
    let child = control
        .open_thread(&card(
            "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            "count",
            Permit::Ask,
        ))
        .unwrap();
    let main = control
        .open_thread(&card(
            "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
            "count",
            Permit::Go,
        ))
        .unwrap();
    let request = requested(control.run(child, &call(1)).await.err().unwrap());
    assert_eq!(request.supervisor, None);
    assert!(matches!(
        control.resolve_review(
            main,
            &request.id,
            ReviewResolution::ApproveOnce {
                note: "fabricated".into()
            }
        ),
        Err(ControlError::ReviewMissing(_))
    ));
    control.set_supervisor(child, main).unwrap();
    let assigned = requested(control.run(child, &call(1)).await.err().unwrap());
    assert_eq!(assigned.supervisor, Some(main));
    assert_ne!(assigned.id, request.id);
}

#[tokio::test]
async fn tool_and_body_review_subjects_cannot_collide() {
    let (_tmp, control, child, main) = setup(Permit::Ask);
    control
        .register(&listed(&[("scene", &["tool:count"])], &[], &[]))
        .unwrap();
    control
        .mount_agent("tool:count", Arc::new(Cautious), &declares(&["scene"], &[]))
        .unwrap();
    let event = control.push_event("scene", "change").unwrap();
    let DeliveryOutcome::NeedsReview(body_request) =
        control.offer_on(child, "tool:count", &event.id).unwrap()
    else {
        panic!("review expected")
    };
    let tool_request = requested(control.run(child, &call(1)).await.err().unwrap());
    assert_ne!(body_request.id, tool_request.id);
    assert_eq!(control.pending_reviews(main).unwrap().len(), 2);
    control
        .resolve_review(
            main,
            &tool_request.id,
            ReviewResolution::ApproveOnce {
                note: "tool only".into(),
            },
        )
        .unwrap();
    assert!(control.run(child, &call(1)).await.is_ok());
    let DeliveryOutcome::NeedsReview(still_waiting) =
        control.offer_on(child, "tool:count", &event.id).unwrap()
    else {
        panic!("body still waiting")
    };
    assert_eq!(still_waiting.id, body_request.id);
}

#[tokio::test]
async fn approval_evidence_does_not_replace_the_source_of_a_remembered_fact() {
    let tmp = TempDir::new("review-memory-source");
    let control = AgentControl::open(tmp.path(), &[("count", count_ctor)]).unwrap();
    let mut child_card = card("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", "count", Permit::Ask);
    child_card.tools[0].tag.memory = crate::act::MemoryFacet::Remember { aspect: None };
    let mut main_card = card("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb", "count", Permit::Go);
    main_card.tools[0].tag.memory = child_card.tools[0].tag.memory.clone();
    let child = control.open_thread(&child_card).unwrap();
    let main = control.open_thread(&main_card).unwrap();
    control.set_supervisor(child, main).unwrap();
    let agent = control.live(child).unwrap();
    let source = control
        .append_event(
            &agent,
            Kind::Observation,
            "source",
            "actual source observation".into(),
            vec![],
        )
        .unwrap();
    let request = requested(control.run(child, &call(1)).await.err().unwrap());
    control
        .resolve_review(
            main,
            &request.id,
            ReviewResolution::ApproveOnce {
                note: "reviewed".into(),
            },
        )
        .unwrap();
    control.run(child, &call(1)).await.unwrap();
    let tape = control.events(child).unwrap();
    let memory = tape
        .iter()
        .find(|e| e.tags.iter().any(|tag| tag == "memory"))
        .unwrap();
    assert_eq!(memory.refs, vec![source]);
    assert!(
        tape.iter()
            .filter(|e| e.tags.iter().any(|tag| tag.starts_with("review_")))
            .all(|e| e.tags.iter().any(|tag| tag == "audit"))
    );
}
