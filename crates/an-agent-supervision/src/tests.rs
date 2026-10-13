use super::*;
use an_agent_semantic::{Capability, Polarity, Resource};

fn instance(principal: u128, incarnation: u128) -> InstanceRef {
    InstanceRef {
        principal: Uuid::from_u128(principal),
        incarnation: Uuid::from_u128(incarnation),
    }
}

fn setup() -> (Supervision, Uuid, Uuid) {
    let mut state = Supervision::default();
    let child = instance(1, 101);
    let main = instance(2, 102);
    state.open(child).unwrap();
    state.open(main).unwrap();
    state.bind(child.principal, main.principal).unwrap();
    (state, child.principal, main.principal)
}

fn request(state: &Supervision, child: Uuid, mount: Option<Uuid>) -> ReviewRequest {
    let scope = state.scope(child, mount).unwrap();
    ReviewRequest {
        id: "review-1".into(),
        child,
        supervisor: scope.supervisor.map(|s| s.principal),
        subject: "tool:write".into(),
        event_id: None,
        attempt: Attempt::new(
            "attempt-1",
            "tool:write",
            Capability::exact(Polarity::Effect, "write", Resource::new("object", ["1"])),
            vec![FactRef::new("generation", "A")],
        ),
        cause: ReviewCause::HighRisk {
            reason: "ask".into(),
        },
        tape_id: "event-1".into(),
        scope: Some(scope),
    }
}

fn call() -> CallBinding {
    CallBinding {
        operation: "write".into(),
        input: "exact bytes".into(),
    }
}

fn approval() -> ReviewResolution {
    ReviewResolution::ApproveOnce {
        note: "checked".into(),
    }
}

#[test]
fn missing_or_stale_scope_never_enters_the_active_ledger() {
    let (mut state, child, _) = setup();
    let original = request(&state, child, None);
    let mut legacy = original.clone();
    legacy.scope = None;
    assert!(matches!(
        state.insert(legacy, Some(call())),
        Err(ReviewError::Stale(_))
    ));
    state.revoke(child).unwrap();
    assert!(matches!(
        state.insert(original, Some(call())),
        Err(ReviewError::Stale(_))
    ));
}

#[test]
fn closing_a_request_prevents_id_resurrection() {
    let (mut state, child, main) = setup();
    let original = request(&state, child, None);
    state.insert(original.clone(), Some(call())).unwrap();
    state
        .resolve(main, &original.id, &approval(), "card")
        .unwrap();
    assert!(
        state
            .check_call(&original.id, &call(), &original.attempt.facts, "card")
            .unwrap()
    );
    assert!(state.close(&original.id).is_some());
    assert!(matches!(
        state.insert(original, Some(call())),
        Err(ReviewError::Binding(_))
    ));
}

#[test]
fn supervisor_retirement_detaches_children_even_after_reopening_the_same_principal() {
    let (mut state, child, main) = setup();
    let original = request(&state, child, None);
    state.insert(original.clone(), Some(call())).unwrap();
    state
        .resolve(main, &original.id, &approval(), "card")
        .unwrap();
    assert_eq!(state.retire(main), vec![original.clone()]);
    state.open(instance(2, 202)).unwrap();
    assert!(state.pending(main).is_empty());
    assert_eq!(state.scope(child, None).unwrap().supervisor, None);
    assert!(!state.current(original.scope.as_ref().unwrap()));
    state.bind(main, child).unwrap();
}

#[test]
fn revocation_advances_epoch_without_dropping_the_relation() {
    let (mut state, child, main) = setup();
    let original = request(&state, child, None);
    state.insert(original.clone(), Some(call())).unwrap();
    assert_eq!(state.revoke(main).unwrap(), vec![original.clone()]);
    let next = state.scope(child, None).unwrap();
    let previous = original.scope.unwrap();
    assert_eq!(next.supervisor, previous.supervisor);
    assert!(next.supervisor_epoch > previous.supervisor_epoch);
    assert!(!state.current(&previous));
}

#[test]
fn only_the_retired_mount_loses_its_requests() {
    let (mut state, child, main) = setup();
    let first = Uuid::from_u128(10);
    let second = Uuid::from_u128(11);
    state.mount(first);
    state.mount(second);
    let original = request(&state, child, Some(first));
    let mut other = request(&state, child, Some(second));
    other.id = "review-2".into();
    state.insert(original.clone(), None).unwrap();
    state.insert(other.clone(), None).unwrap();
    assert_eq!(state.unmount(first), vec![original]);
    assert_eq!(state.pending(main), vec![other]);
}

#[test]
fn generation_restoration_does_not_restore_a_closed_approval() {
    let (mut state, child, main) = setup();
    let original = request(&state, child, None);
    state.insert(original.clone(), Some(call())).unwrap();
    state
        .resolve(main, &original.id, &approval(), "card")
        .unwrap();
    assert!(state.invalidate_fact("generation", "A").is_empty());
    assert_eq!(state.invalidate_fact("generation", "B"), vec![original]);
    assert!(state.invalidate_fact("generation", "A").is_empty());
    assert!(state.pending(main).is_empty());
}

#[test]
fn approval_requires_a_reviewable_call_and_the_bound_supervisor() {
    let (mut state, child, main) = setup();
    let mut original = request(&state, child, None);
    state.insert(original.clone(), None).unwrap();
    assert!(matches!(
        state.resolve(child, &original.id, &approval(), "card"),
        Err(ReviewError::Authority(_))
    ));
    assert!(matches!(
        state.resolve(main, &original.id, &approval(), "card"),
        Err(ReviewError::Ceiling(_))
    ));
    state.close(&original.id);
    original.id = "review-2".into();
    original.cause = ReviewCause::PermissionDenied {
        reason: "denied".into(),
    };
    state.insert(original.clone(), Some(call())).unwrap();
    assert!(matches!(
        state.resolve(main, &original.id, &approval(), "card"),
        Err(ReviewError::Ceiling(_))
    ));
}

#[test]
fn exact_inputs_and_approver_card_are_checked_and_consult_revokes() {
    let (mut state, child, main) = setup();
    let original = request(&state, child, None);
    state.insert(original.clone(), Some(call())).unwrap();
    let changed = CallBinding {
        input: "different bytes".into(),
        ..call()
    };
    assert!(matches!(
        state.check_call(&original.id, &changed, &original.attempt.facts, "card"),
        Err(ReviewError::Stale(_))
    ));
    state
        .resolve(main, &original.id, &approval(), "card")
        .unwrap();
    assert!(matches!(
        state.check_call(&original.id, &call(), &original.attempt.facts, "new-card"),
        Err(ReviewError::Stale(_))
    ));
    assert!(matches!(
        state.resolve(main, &original.id, &approval(), "card"),
        Err(ReviewError::Authority(_))
    ));
    state
        .resolve(
            main,
            &original.id,
            &ReviewResolution::Consult {
                note: "ask for advice".into(),
            },
            "card",
        )
        .unwrap();
    assert!(
        !state
            .check_call(&original.id, &call(), &original.attempt.facts, "card")
            .unwrap()
    );
}
