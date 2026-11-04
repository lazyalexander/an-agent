//! Failed durable lifecycle transitions must converge without reviving authority.
use super::*;
use std::sync::atomic::Ordering;

#[test]
fn failed_open_and_resume_leave_no_phantom_seat_or_writer() {
    let tmp = TempDir::new("failed-lifecycle");
    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    let spec = card("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", "bash", Permit::Deny);
    control.register(&listed(&[], &[], &[])).unwrap();
    control
        .inner
        .fail_lifecycle_append
        .store(true, Ordering::Relaxed);
    assert!(control.open_agent(&spec).is_err());
    assert!(control.inner.pool.tree().get(spec.id).is_none());
    assert!(
        !control
            .inner
            .agent_runs
            .lock()
            .unwrap()
            .contains_key(&spec.id)
    );
    control.open_agent(&spec).unwrap();
    let manifest = control.seal_session(spec.id).unwrap();
    control.finish(spec.id, "done").unwrap();
    control.release(spec.id).unwrap();
    control
        .inner
        .fail_lifecycle_append
        .store(true, Ordering::Relaxed);
    assert!(control.resume_session(&spec, manifest.id).is_err());
    assert!(control.inner.pool.tree().get(spec.id).is_none());
    control.resume_session(&spec, manifest.id).unwrap();
}

#[test]
fn failed_finish_intent_is_open_but_failed_final_note_only_allows_finish_retry() {
    let tmp = TempDir::new("failed-finish");
    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    let spec = card("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", "bash", Permit::Deny);
    let id = control.open_agent(&spec).unwrap();
    let agent = control.mounted(id).unwrap();
    agent.session().tape().fail_next_append();
    assert!(control.finish(id, "original conclusion").is_err());
    assert!(control.begin_beat(id).is_ok());
    agent.session().tape().fail_append_after(1);
    assert!(control.finish(id, "original conclusion").is_err());
    assert!(matches!(
        control.begin_beat(id),
        Err(ControlError::Finishing(_))
    ));
    assert!(matches!(
        control.release(id),
        Err(ControlError::NotFinished(_))
    ));
    control
        .finish(id, "must not replace pending conclusion")
        .unwrap();
    let notes = control.events(id).unwrap();
    let finish = notes
        .iter()
        .find(|e| e.tags.iter().any(|t| t == "finish"))
        .unwrap();
    assert!(finish.content.contains("original conclusion"));
    control.release(id).unwrap();
    control.open_agent(&spec).unwrap();
}

#[test]
fn resume_of_an_incomplete_finish_can_only_complete_its_original_conclusion() {
    let tmp = TempDir::new("resume-finishing");
    let spec = card("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", "bash", Permit::Deny);
    let manifest = {
        let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
        let id = control.open_agent(&spec).unwrap();
        let manifest = control.seal_session(id).unwrap();
        control
            .mounted(id)
            .unwrap()
            .session()
            .tape()
            .fail_append_after(1);
        assert!(control.finish(id, "persisted conclusion").is_err());
        manifest
    };
    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    control.resume_session(&spec, manifest.id).unwrap();
    assert!(matches!(
        control.begin_beat(spec.id),
        Err(ControlError::Finishing(_))
    ));
    control.finish(spec.id, "different conclusion").unwrap();
    assert!(control.events(spec.id).unwrap().iter().any(
        |e| e.tags.iter().any(|t| t == "finish") && e.content.contains("persisted conclusion")
    ));
    control.release(spec.id).unwrap();
}
