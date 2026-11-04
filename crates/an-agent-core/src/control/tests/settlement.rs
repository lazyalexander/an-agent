//! Retain siblings and published-but-withheld notes without repeating effects.
use super::supervision::setup;
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Model(Arc<AtomicUsize>);
impl ModelClient for Model {
    fn complete(&self, _: &ModelSpec, _: Vec<ModelMessage>) -> Result<Completion, String> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(Completion {
            content: "model observation".into(),
            usage: None,
        })
    }
}
struct Think;
impl AgentBody for Think {
    fn evaluate(&self, projection: &Value) -> Result<BeatStep, String> {
        if projection["steps"] == 0 {
            Ok(BeatStep::InvokeModel { clips: vec![] })
        } else {
            Ok(BeatStep::Utter {
                text: "first reply".into(),
            })
        }
    }
}
struct Counted(Arc<AtomicUsize>);
impl SpoolBeat for Counted {
    fn receive(&self, _: &WorkspaceRecord) -> Result<String, String> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok("a published reply".into())
    }
}
struct RevokeSecond(Mutex<Option<AgentControl>>);
impl HookRunner for RevokeSecond {
    fn run_before(&self, _: &HookHandler, _: &WorkspaceRecord) -> Result<HookVerdict, String> {
        Ok(HookVerdict::Allow)
    }
    fn run_after(&self, _: &HookHandler, reply: &SpoolReply) -> Result<HookVerdict, String> {
        if reply.spool == "second" {
            self.0
                .lock()
                .unwrap()
                .take()
                .unwrap()
                .register(&listed(&[("scene", &[])], &["main"], &[]))
                .unwrap();
        }
        Ok(HookVerdict::Allow)
    }
}
#[test]
fn later_revocation_retains_prior_reply_and_fences_whole_event_replay() {
    let (tmp, control, child, _) = setup(Permit::Go);
    let calls = Arc::new(AtomicUsize::new(0));
    control
        .register(&listed(&[("scene", &["first", "second"])], &["main"], &[]))
        .unwrap();
    control
        .put_config(
            "main",
            b"[hooks]\nafter = [{ type = \"mcp\", server = \"guard\", tool = \"check\" }]\n",
        )
        .unwrap();
    control.set_model_client(Arc::new(Model(calls.clone())));
    control
        .mount_agent(
            "first",
            Arc::new(Think),
            &declares_model(&["scene"], &[], crate::act::ModelFacet::Complete),
        )
        .unwrap();
    let second_calls = Arc::new(AtomicUsize::new(0));
    control
        .mount_spool(
            "second",
            Arc::new(Counted(second_calls.clone())),
            &declares(&["scene"], &[]),
        )
        .unwrap();
    control.set_hook_runner(Arc::new(RevokeSecond(Mutex::new(Some(control.clone())))));
    let event = control.push_event("scene", "change").unwrap();
    let ControlError::DeliveryInterrupted {
        event: failed,
        ready,
        withheld,
        source,
    } = control.dispatch_on(child, &event.id).unwrap_err()
    else {
        panic!("partial settlement required")
    };
    assert_eq!(failed, event.id);
    assert!(matches!(*source, ControlError::ReviewStale(_)));
    assert_eq!(ready.len(), 1);
    let first = ready[0].reply().unwrap();
    assert_eq!(first.spool, "first");
    assert_eq!(withheld.unwrap().reply().unwrap().spool, "second");
    assert!(
        control
            .events(child)
            .unwrap()
            .iter()
            .any(|e| e.id == first.tape_id)
    );
    // Retention does not authorize landing after revocation.
    assert!(control.validate_reply(child, first).is_err());
    assert!(matches!(
        control.dispatch_on(child, &event.id),
        Err(ControlError::DeliveryUnsettled(_))
    ));
    assert!(matches!(
        control.offer_on(child, "first", &event.id),
        Err(ControlError::DeliveryUnsettled(_))
    ));
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(second_calls.load(Ordering::Relaxed), 1);
    let manifest = control.seal_session(child).unwrap();
    drop(control);
    let reopened =
        AgentControl::open(tmp.path(), &[("count", super::supervision::count_ctor)]).unwrap();
    let spec = card("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", "count", Permit::Go);
    reopened.resume_session(&spec, manifest.id).unwrap();
    assert!(matches!(
        reopened.offer_on(child, "first", &event.id),
        Err(ControlError::DeliveryUnsettled(_))
    ));
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}

struct FailSettlement(Mutex<Option<(AgentControl, Uuid)>>);
impl HookRunner for FailSettlement {
    fn run_before(&self, _: &HookHandler, _: &WorkspaceRecord) -> Result<HookVerdict, String> {
        Ok(HookVerdict::Allow)
    }
    fn run_after(&self, _: &HookHandler, _: &SpoolReply) -> Result<HookVerdict, String> {
        let (control, id) = self.0.lock().unwrap().take().unwrap();
        control
            .mounted(id)
            .unwrap()
            .session()
            .tape()
            .fail_next_append();
        Ok(HookVerdict::Allow)
    }
}
#[test]
fn failed_settlement_write_retains_ready_and_fences_even_a_visible_completion_line() {
    let (tmp, control, child, _) = setup(Permit::Go);
    let calls = Arc::new(AtomicUsize::new(0));
    control
        .register(&listed(&[("scene", &["body"])], &["main"], &[]))
        .unwrap();
    control
        .put_config(
            "main",
            b"[hooks]\nafter = [{ type = \"mcp\", server = \"guard\", tool = \"check\" }]\n",
        )
        .unwrap();
    control
        .mount_spool(
            "body",
            Arc::new(Counted(calls.clone())),
            &declares(&["scene"], &[]),
        )
        .unwrap();
    control.set_hook_runner(Arc::new(FailSettlement(Mutex::new(Some((
        control.clone(),
        child,
    ))))));
    let event = control.push_event("scene", "work").unwrap();
    let ControlError::DeliveryInterrupted {
        ready,
        withheld,
        source,
        ..
    } = control.offer_on(child, "body", &event.id).unwrap_err()
    else {
        panic!("failed settlement must retain progress")
    };
    assert_eq!(ready.len(), 1);
    assert!(withheld.is_none());
    assert!(matches!(*source, ControlError::Store(_)));
    // Model the complete line which a real failed sync may leave readable.
    let instance = control
        .inner
        .supervision
        .lock()
        .unwrap()
        .instance(child)
        .unwrap();
    control
        .append_audit_event(
            &control.mounted(child).unwrap(),
            Kind::Action,
            "delivery_batch",
            json!({"event":event.id,"instance":instance,"status":"settled"}).to_string(),
            vec![],
        )
        .unwrap();
    assert!(matches!(
        control.offer_on(child, "body", &event.id),
        Err(ControlError::DeliveryUnsettled(_))
    ));
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    let manifest = control.seal_session(child).unwrap();
    drop(control);
    let reopened =
        AgentControl::open(tmp.path(), &[("count", super::supervision::count_ctor)]).unwrap();
    let spec = card("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", "count", Permit::Go);
    reopened.resume_session(&spec, manifest.id).unwrap();
    assert!(matches!(
        reopened.offer_on(child, "body", &event.id),
        Err(ControlError::DeliveryUnsettled(_))
    ));
}
