//! Real boundary paths for incarnation, generation and revocation changes.

use super::supervision::{Cautious, call, count_ctor, requested, setup};
use super::*;

struct Fresh;
impl AgentBody for Fresh {
    fn evaluate(&self, _: &Value) -> Result<BeatStep, String> {
        Ok(BeatStep::Utter {
            text: "fresh body".into(),
        })
    }
}

fn approve(control: &AgentControl, main: Uuid, request: &ReviewRequest) {
    control
        .resolve_review(
            main,
            &request.id,
            ReviewResolution::ApproveOnce {
                note: "one exact attempt".into(),
            },
        )
        .unwrap();
}

fn no_execution(control: &AgentControl, child: Uuid) {
    assert!(
        !control
            .events(child)
            .unwrap()
            .iter()
            .any(|event| event.act.is_some())
    );
}

#[test]
fn remount_closes_only_the_old_mounts_reviews_and_notifies_both_tapes() {
    let (_tmp, control, child, main) = setup(Permit::Go);
    control
        .register(&listed(&[("scene", &["body", "other"])], &[], &[]))
        .unwrap();
    for name in ["body", "other"] {
        control
            .mount_agent(name, Arc::new(Cautious), &declares(&["scene"], &[]))
            .unwrap();
    }
    let event = control.push_event("scene", "change").unwrap();
    let outcomes = control.dispatch_on(child, &event.id).unwrap();
    let DeliveryOutcome::NeedsReview(old) = &outcomes[0] else {
        panic!("review expected")
    };
    control.unmount_spool("body").unwrap();
    assert_eq!(control.pending_reviews(main).unwrap().len(), 1);
    control
        .mount_agent("body", Arc::new(Fresh), &declares(&["scene"], &[]))
        .unwrap();
    assert_eq!(
        control
            .offer_on(child, "body", &event.id)
            .unwrap()
            .reply()
            .unwrap()
            .reply,
        "fresh body"
    );
    for recipient in [child, main] {
        assert!(control.events(recipient).unwrap().iter().any(|event| {
            event.tags.iter().any(|tag| tag == "review_closed") && event.content.contains(&old.id)
        }));
    }
    assert!(matches!(
        control.resolve_review(
            main,
            &old.id,
            ReviewResolution::Retry {
                note: "late".into()
            }
        ),
        Err(ControlError::ReviewMissing(_))
    ));
}

#[tokio::test]
async fn reopened_or_resumed_supervisor_cannot_inherit_old_approval_or_bindings() {
    for resume in [false, true] {
        let (_tmp, control, child, main) = setup(Permit::Ask);
        let request = requested(control.run(child, &call(1)).await.err().unwrap());
        approve(&control, main, &request);
        let manifest = control.seal_session(main).unwrap();
        control.finish(main, "done").unwrap();
        control.release(main).unwrap();
        let main_card = card("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb", "count", Permit::Go);
        if resume {
            control.resume_session(&main_card, manifest.id).unwrap();
        } else {
            control.open_thread(&main_card).unwrap();
        }
        assert!(control.pending_reviews(main).unwrap().is_empty());
        let unassigned = requested(control.run(child, &call(1)).await.err().unwrap());
        assert_eq!(unassigned.supervisor, None);
        control.set_supervisor(child, main).unwrap();
        let fresh = requested(control.run(child, &call(1)).await.err().unwrap());
        assert_ne!(
            fresh.scope.as_ref().unwrap().supervisor,
            request.scope.as_ref().unwrap().supervisor
        );
        no_execution(&control, child);
        approve(&control, main, &fresh);
        assert!(control.run(child, &call(1)).await.is_ok());
    }
}

#[tokio::test]
async fn child_recreation_drops_old_supervisor_edges_and_inbox_entries() {
    let (_tmp, control, child, main) = setup(Permit::Ask);
    let old = requested(control.run(child, &call(1)).await.err().unwrap());
    let manifest = control.seal_session(child).unwrap();
    control.finish(child, "done").unwrap();
    control.release(child).unwrap();
    control
        .resume_session(
            &card("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", "count", Permit::Ask),
            manifest.id,
        )
        .unwrap();
    assert!(control.pending_reviews(main).unwrap().is_empty());
    let fresh = requested(control.run(child, &call(1)).await.err().unwrap());
    assert_eq!(fresh.supervisor, None);
    assert_ne!(old.scope.unwrap().child, fresh.scope.unwrap().child);
    // The old child-to-main edge must not make this fresh edge cyclic.
    control.set_supervisor(main, child).unwrap();
}

#[tokio::test]
async fn changing_back_to_identical_bytes_never_resurrects_approval() {
    for ingredient in ["config", "env", "registration"] {
        let (_tmp, control, child, main) = setup(Permit::Ask);
        let registration = listed(&[], &["main"], &["main"]);
        control.register(&registration).unwrap();
        control
            .put_config("main", b"[guidance]\nrules = [\"A\"]\n")
            .unwrap();
        control.put_env("main", "A").unwrap();
        let old = requested(control.run(child, &call(1)).await.err().unwrap());
        approve(&control, main, &old);
        match ingredient {
            "config" => {
                control
                    .put_config("main", b"[guidance]\nrules = [\"B\"]\n")
                    .unwrap();
                control
                    .put_config("main", b"[guidance]\nrules = [\"A\"]\n")
                    .unwrap();
            }
            "env" => {
                control.put_env("main", "B").unwrap();
                control.put_env("main", "A").unwrap();
            }
            _ => {
                control
                    .register(&listed(&[("extra", &[])], &["main"], &["main"]))
                    .unwrap();
                control.register(&registration).unwrap();
            }
        }
        assert!(control.pending_reviews(main).unwrap().is_empty());
        let fresh = requested(control.run(child, &call(1)).await.err().unwrap());
        assert_ne!(old.id, fresh.id);
        no_execution(&control, child);
    }
}

#[tokio::test]
async fn identical_config_write_and_unrelated_mount_do_not_revoke_approval() {
    let (_tmp, control, child, main) = setup(Permit::Ask);
    let registration = listed(&[("scene", &["body"])], &["main"], &[]);
    control.register(&registration).unwrap();
    control
        .put_config("main", b"[guidance]\nrules = []\n")
        .unwrap();
    let request = requested(control.run(child, &call(1)).await.err().unwrap());
    approve(&control, main, &request);
    control.register(&registration).unwrap();
    control
        .put_config("main", b"[guidance]\nrules = []\n")
        .unwrap();
    control
        .mount_agent("body", Arc::new(Fresh), &declares(&["scene"], &[]))
        .unwrap();
    control.unmount_spool("body").unwrap();
    assert!(control.run(child, &call(1)).await.is_ok());
}

#[tokio::test]
async fn explicit_revocation_advances_scope_without_erasing_the_binding() {
    let (_tmp, control, child, main) = setup(Permit::Ask);
    let old = requested(control.run(child, &call(1)).await.err().unwrap());
    approve(&control, main, &old);
    control.revoke_reviews(main, "authority withdrawn").unwrap();
    let fresh = requested(control.run(child, &call(1)).await.err().unwrap());
    assert_eq!(fresh.supervisor, Some(main));
    assert!(
        fresh.scope.as_ref().unwrap().supervisor_epoch
            > old.scope.as_ref().unwrap().supervisor_epoch
    );
    no_execution(&control, child);
}

#[tokio::test]
async fn stale_unapproved_input_closes_the_old_request_instead_of_waiting_forever() {
    let (_tmp, control, child, main) = setup(Permit::Ask);
    let old = requested(control.run(child, &call(1)).await.err().unwrap());
    assert!(matches!(
        control.run(child, &call(2)).await,
        Err(ControlError::ReviewStale(_))
    ));
    assert!(control.pending_reviews(main).unwrap().is_empty());
    let fresh = requested(control.run(child, &call(2)).await.err().unwrap());
    assert_ne!(old.id, fresh.id);
    no_execution(&control, child);
}

#[tokio::test]
async fn failed_consult_evidence_still_revokes_the_unused_approval() {
    let (_tmp, control, child, main) = setup(Permit::Ask);
    let request = requested(control.run(child, &call(1)).await.err().unwrap());
    approve(&control, main, &request);
    control
        .live(child)
        .unwrap()
        .session()
        .tape()
        .fail_next_append();
    assert!(
        control
            .resolve_review(
                main,
                &request.id,
                ReviewResolution::Consult {
                    note: "withdraw".into()
                }
            )
            .is_err()
    );
    assert_eq!(
        requested(control.run(child, &call(1)).await.err().unwrap()).id,
        request.id
    );
    no_execution(&control, child);
}

#[tokio::test]
async fn failed_consumption_evidence_burns_the_approval_without_execution() {
    let (_tmp, control, child, main) = setup(Permit::Ask);
    let old = requested(control.run(child, &call(1)).await.err().unwrap());
    approve(&control, main, &old);
    control
        .live(child)
        .unwrap()
        .session()
        .tape()
        .fail_next_append();
    assert!(matches!(
        control.run(child, &call(1)).await,
        Err(ControlError::Store(_))
    ));
    assert!(control.pending_reviews(main).unwrap().is_empty());
    let fresh = requested(control.run(child, &call(1)).await.err().unwrap());
    assert_ne!(fresh.id, old.id);
    no_execution(&control, child);
}

#[tokio::test]
async fn retirement_revokes_every_request_even_when_one_tape_fails() {
    let (_tmp, control, child, main) = setup(Permit::Ask);
    let request = requested(control.run(child, &call(1)).await.err().unwrap());
    approve(&control, main, &request);
    control
        .live(child)
        .unwrap()
        .session()
        .tape()
        .fail_next_append();
    assert!(control.finish(main, "retire").is_err());
    control.release(main).unwrap();
    control
        .open_thread(&card(
            "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
            "count",
            Permit::Go,
        ))
        .unwrap();
    assert!(control.pending_reviews(main).unwrap().is_empty());
    assert_eq!(
        requested(control.run(child, &call(1)).await.err().unwrap()).supervisor,
        None
    );
    no_execution(&control, child);
}

struct ReplaceWhileRunning {
    control: AgentControl,
    fail: bool,
}
impl AgentBody for ReplaceWhileRunning {
    fn evaluate(&self, _: &Value) -> Result<BeatStep, String> {
        self.control.unmount_spool("body").unwrap();
        self.control
            .mount_agent("body", Arc::new(Fresh), &declares(&["scene"], &[]))
            .unwrap();
        if self.fail {
            Err("old body failed".into())
        } else {
            Ok(BeatStep::Utter {
                text: "stale reply".into(),
            })
        }
    }
}

#[test]
fn an_old_callback_cannot_remove_its_replacement_or_publish_a_stale_reply() {
    for fail in [false, true] {
        let (_tmp, control, child, _) = setup(Permit::Go);
        control
            .register(&listed(&[("scene", &["body"])], &[], &[]))
            .unwrap();
        control
            .mount_agent(
                "body",
                Arc::new(ReplaceWhileRunning {
                    control: control.clone(),
                    fail,
                }),
                &declares(&["scene"], &[]),
            )
            .unwrap();
        let event = control.push_event("scene", "change").unwrap();
        assert!(control.offer_on(child, "body", &event.id).is_err());
        assert_eq!(
            control
                .offer_on(child, "body", &event.id)
                .unwrap()
                .reply()
                .unwrap()
                .reply,
            "fresh body"
        );
        assert!(!control.events(child).unwrap().iter().any(|e| {
            e.tags.iter().any(|tag| tag == "applied") || e.content.contains("stale reply")
        }));
    }
}

#[test]
fn a_new_control_process_does_not_restore_approval_from_the_same_session_tape() {
    let tmp = TempDir::new("review-restart");
    let child_card = card("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", "count", Permit::Ask);
    let control = AgentControl::open(tmp.path(), &[("count", count_ctor)]).unwrap();
    let child = control.open_thread(&child_card).unwrap();
    let manifest = control.seal_session(child).unwrap();
    let old_instance = control
        .inner
        .supervision
        .lock()
        .unwrap()
        .instance(child)
        .unwrap();
    drop(control);
    let next = AgentControl::open(tmp.path(), &[("count", count_ctor)]).unwrap();
    next.resume_session(&child_card, manifest.id).unwrap();
    let new_instance = next
        .inner
        .supervision
        .lock()
        .unwrap()
        .instance(child)
        .unwrap();
    assert_ne!(old_instance, new_instance);
}
