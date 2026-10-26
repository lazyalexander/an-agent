//! Registration revocation at real body, publication and host landing boundaries.
use super::supervision::setup;
use super::*;

struct Reply;
impl SpoolBeat for Reply {
    fn receive(&self, _: &WorkspaceRecord) -> Result<String, String> {
        Ok(r#"{"utter":"visible","raise":[{"event":"next","body":"payload"}]}"#.into())
    }
}
fn routed() -> Registration {
    listed(&[("scene", &["body"]), ("next", &[])], &[], &[])
}
fn revoked() -> Registration {
    listed(&[("scene", &[]), ("next", &[])], &[], &[])
}
fn mount(control: &AgentControl) {
    control.register(&routed()).unwrap();
    control
        .mount_spool("body", Arc::new(Reply), &declares(&["scene"], &["next"]))
        .unwrap();
}
fn replies(control: &AgentControl, child: Uuid) -> SpoolReply {
    let event = control.push_event("scene", "change").unwrap();
    said(control.offer_on(child, "body", &event.id).unwrap())
}
fn count_tag(control: &AgentControl, id: Uuid, tag: &str) -> usize {
    control
        .events(id)
        .unwrap()
        .iter()
        .filter(|e| e.tags.iter().any(|t| t == tag))
        .count()
}
struct Blocked {
    started: std::sync::mpsc::Sender<()>,
    resume: Mutex<std::sync::mpsc::Receiver<()>>,
}
impl SpoolBeat for Blocked {
    fn receive(&self, _: &WorkspaceRecord) -> Result<String, String> {
        self.started.send(()).map_err(|e| e.to_string())?;
        self.resume
            .lock()
            .unwrap()
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        Reply.receive(&WorkspaceRecord {
            id: String::new(),
            kind: String::new(),
            body: None,
            sha256: None,
            at: None,
            name: None,
        })
    }
}
#[test]
fn registration_replaced_while_body_runs_fences_reply() {
    let (_tmp, control, child, _) = setup(Permit::Go);
    control.register(&routed()).unwrap();
    let (started, ready) = std::sync::mpsc::channel();
    let (resume, wait) = std::sync::mpsc::channel();
    control
        .mount_spool(
            "body",
            Arc::new(Blocked {
                started,
                resume: Mutex::new(wait),
            }),
            &declares(&["scene"], &["next"]),
        )
        .unwrap();
    let event = control.push_event("scene", "change").unwrap();
    let worker = control.clone();
    let task = std::thread::spawn(move || worker.offer_on(child, "body", &event.id));
    ready
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    control.register(&revoked()).unwrap();
    resume.send(()).unwrap();
    assert!(matches!(
        task.join().unwrap(),
        Err(ControlError::ReviewStale(_))
    ));
    assert_eq!(count_tag(&control, child, "spool"), 0);
    assert_eq!(count_tag(&control, child, "applied"), 0);
}
#[test]
fn revoked_reply_cannot_be_confirmed_as_current() {
    let (_tmp, control, child, _) = setup(Permit::Go);
    mount(&control);
    let reply = replies(&control, child);
    control.register(&revoked()).unwrap();
    assert!(matches!(
        control.applied(child, &reply, "host claims a prior landing"),
        Err(ControlError::ReviewStale(_))
    ));
    assert_eq!(count_tag(&control, child, "applied"), 0);
}

#[test]
fn identical_registration_stays_valid_but_aba_and_missing_facts_do_not() {
    let (_tmp, control, child, _) = setup(Permit::Go);
    mount(&control);
    let mut reply = replies(&control, child);
    control.register(&routed()).unwrap();
    control.validate_reply(child, &reply).unwrap();
    control.applied(child, &reply, "current landing").unwrap();
    control.register(&revoked()).unwrap();
    control.register(&routed()).unwrap();
    assert!(matches!(
        control.validate_reply(child, &reply),
        Err(ControlError::ReviewStale(_))
    ));
    reply = replies(&control, child);
    reply
        .delivery_facts
        .retain(|f| f.kind != "workspace.registration");
    assert!(matches!(
        control.validate_reply(child, &reply),
        Err(ControlError::ReviewStale(_))
    ));
}

#[test]
fn reopen_does_not_resurrect_a_previous_delivery_grant() {
    let (tmp, control, child, _) = setup(Permit::Go);
    mount(&control);
    let reply = replies(&control, child);
    let manifest = control.seal_session(child).unwrap();
    drop(control);
    let reopened =
        AgentControl::open(tmp.path(), &[("count", super::supervision::count_ctor)]).unwrap();
    let spec = card("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", "count", Permit::Go);
    reopened.resume_session(&spec, manifest.id).unwrap();
    reopened
        .mount_spool("body", Arc::new(Reply), &declares(&["scene"], &["next"]))
        .unwrap();
    assert!(matches!(
        reopened.validate_reply(child, &reply),
        Err(ControlError::ReviewStale(_))
    ));
    assert!(matches!(
        reopened.applied(child, &reply, "late prior claim"),
        Err(ControlError::ReviewStale(_))
    ));
    assert_eq!(count_tag(&reopened, child, "applied_stale"), 1);
    let fresh = replies(&reopened, child);
    reopened.applied(child, &fresh, "fresh landing").unwrap();
}

struct RevokeAfterModel(AgentControl);
impl AgentBody for RevokeAfterModel {
    fn evaluate(&self, projection: &Value) -> Result<BeatStep, String> {
        if projection["steps"] == 0 {
            return Ok(BeatStep::InvokeModel { clips: vec![] });
        }
        self.0.register(&revoked()).map_err(|e| e.to_string())?;
        Ok(BeatStep::Utter {
            text: "must not publish".into(),
        })
    }
}
struct Model;
impl ModelClient for Model {
    fn complete(&self, _: &ModelSpec, _: Vec<ModelMessage>) -> Result<Completion, String> {
        Ok(Completion {
            content: "valid model observation".into(),
            usage: None,
        })
    }
}
#[test]
fn registration_change_after_model_completion_fences_final_utterance() {
    let (_tmp, control, child, _) = setup(Permit::Go);
    control.register(&routed()).unwrap();
    control.set_model_client(Arc::new(Model));
    control
        .mount_agent(
            "body",
            Arc::new(RevokeAfterModel(control.clone())),
            &declares_model(&["scene"], &["next"], crate::act::ModelFacet::Complete),
        )
        .unwrap();
    let event = control.push_event("scene", "change").unwrap();
    assert!(matches!(
        control.offer_on(child, "body", &event.id),
        Err(ControlError::ReviewStale(_))
    ));
    let events = control.events(child).unwrap();
    let observation = events
        .iter()
        .filter_map(|e| serde_json::from_str::<Value>(&e.content).ok())
        .find(|v| v.get("response_sha256").is_some())
        .unwrap();
    let sha = observation["response_sha256"].as_str().unwrap();
    assert_eq!(
        std::fs::read(control.directory(child).unwrap().join("blobs").join(sha)).unwrap(),
        b"valid model observation"
    );
    control.unmount_spool("body").unwrap();
    assert_eq!(count_tag(&control, child, "spool"), 0);
}

struct RevokeHook(Mutex<Option<AgentControl>>);
impl HookRunner for RevokeHook {
    fn run_before(&self, _: &HookHandler, _: &WorkspaceRecord) -> Result<HookVerdict, String> {
        Ok(HookVerdict::Allow)
    }
    fn run_after(&self, _: &HookHandler, _: &SpoolReply) -> Result<HookVerdict, String> {
        self.0
            .lock()
            .unwrap()
            .take()
            .unwrap()
            .register(&revoked())
            .map_err(|e| e.to_string())?;
        Ok(HookVerdict::Allow)
    }
}
#[test]
fn after_hook_cannot_release_a_revoked_reply_to_host() {
    for dispatch in [false, true] {
        let (_tmp, control, child, _) = setup(Permit::Go);
        let mut registration = routed();
        registration.config.push("main".into());
        control.register(&registration).unwrap();
        control
            .put_config(
                "main",
                b"[hooks]\nafter = [{ type = \"mcp\", server = \"guard\", tool = \"check\" }]\n",
            )
            .unwrap();
        control
            .mount_spool("body", Arc::new(Reply), &declares(&["scene"], &["next"]))
            .unwrap();
        control.set_hook_runner(Arc::new(RevokeHook(Mutex::new(Some(control.clone())))));
        let event = control.push_event("scene", "change").unwrap();
        let result = if dispatch {
            control.dispatch_on(child, &event.id).map(|_| ())
        } else {
            control.offer_on(child, "body", &event.id).map(|_| ())
        };
        assert!(matches!(result, Err(ControlError::ReviewStale(_))));
        assert_eq!(count_tag(&control, child, "applied"), 0);
    }
}

#[test]
fn landing_evidence_write_failure_never_reports_success_or_retries() {
    let (_tmp, control, child, _) = setup(Permit::Go);
    mount(&control);
    let reply = replies(&control, child);
    let agent = control.mounted(child).unwrap();
    agent.session().tape().fail_next_append();
    assert!(
        control
            .applied(child, &reply, "claim whose write fails")
            .is_err()
    );
    assert_eq!(count_tag(&control, child, "applied"), 0);
    control.register(&revoked()).unwrap();
    agent.session().tape().fail_next_append();
    assert!(
        control
            .applied(child, &reply, "stale claim whose audit fails")
            .is_err()
    );
    assert_eq!(count_tag(&control, child, "applied_stale"), 0);
    assert!(matches!(
        control.applied(child, &reply, "explicitly resubmitted claim"),
        Err(ControlError::ReviewStale(_))
    ));
    assert_eq!(count_tag(&control, child, "applied_stale"), 1);
    assert_eq!(count_tag(&control, child, "applied"), 0);
}
