use super::*;
struct Fake<'a> {
    cancelled: &'a AtomicBool,
    failure: &'static str,
    steps: Vec<String>,
    stopped: bool,
}
fn ok(bytes: Vec<u8>) -> sandbox::Execution {
    sandbox::Execution {
        status: "succeeded",
        code: Some(0),
        stdout: bytes,
        stderr: vec![],
        pid: Some(7),
        reaped: true,
        group_cleanup_required: false,
    }
}
impl Transport for Fake<'_> {
    fn call(
        &mut self,
        step: &str,
        _args: &[String],
        cancel: &AtomicBool,
        _seconds: u64,
    ) -> Result<sandbox::Execution> {
        self.steps.push(step.into());
        if matches!(step, "stop" | "confirm-stop") {
            assert!(!cancel.load(Ordering::Relaxed), "cleanup was cancelled");
        }
        if step == "stop" {
            if self.failure == "stop" {
                return Err("gateway unreachable".into());
            }
            self.stopped = true;
        }
        if step == "execute" && self.failure == "timeout" {
            let mut result = ok(vec![]);
            result.status = "timed_out";
            result.code = None;
            return Ok(result);
        }
        if step == "candidate" && self.failure == "cancel" {
            self.cancelled.store(true, Ordering::Relaxed);
        }
        if step == "candidate" && self.failure == "oversize" {
            let mut result = ok(vec![b'x'; 65536]);
            result.status = "output_limit";
            return Ok(result);
        }
        if step == "create" && self.failure == "create" {
            return Err("transport lost after create".into());
        }
        Ok(match step {
            "gateway" => ok(br#"{"version":"0.1.5","compute_drivers":[{"name":"vm"}]}"#.to_vec()),
            "version" => ok(b"openshell 0.1.5\n".to_vec()),
            "ready" | "verify-owner" | "confirm-stop" => ok(serde_json::to_vec(&json!({
                "name":"an-test","id":if step=="confirm-stop" && self.failure=="identity" {"replacement"} else {"original"},
                "phase":if self.stopped {"Stopped"} else {"Ready"},
                "annotations":{"an-agent.run":if self.failure=="owner" {"another-run"} else {"run"},"internal.openshell.ai/runtime-generation":if step=="confirm-stop" && self.failure=="generation" {"replacement"} else {"generation-1"}},
                "policy":{"landlock":{"compatibility":"hard_requirement"}}
            }))?),
            "candidate" => ok(b"alpha\nbeta\n".to_vec()),
            _ => ok(vec![]),
        })
    }
}
fn fixture(failure: &'static str) -> (RemoteExecution, Vec<String>) {
    let cancelled = AtomicBool::new(false);
    let profile = Profile {
        cli: "/test/openshell".into(),
        cli_sha256: "0".repeat(64),
        config_home: "/test/config".into(),
        gateway: "test".into(),
        image: format!("test@sha256:{}", "0".repeat(64)),
        wall_seconds: 1,
        control_seconds: 5,
    };
    let mut session = Session {
        transport: Fake {
            cancelled: &cancelled,
            failure,
            steps: vec![],
            stopped: false,
        },
        cancelled: &cancelled,
        seconds: 5,
        evidence: vec![],
    };
    let result = drive(
        &mut session,
        "an-test",
        &profile,
        Path::new("/test/program"),
        Path::new("/test/run"),
    );
    (result, session.transport.steps)
}
#[test]
fn successful_candidate_requires_stop_confirmation() {
    let (result, steps) = fixture("");
    assert_eq!(result.execution.status, "succeeded");
    assert_eq!(result.artifact, Some(b"alpha\nbeta\n".to_vec()));
    assert_eq!(&steps[steps.len() - 2..], ["stop", "confirm-stop"]);
}
#[test]
fn timeout_never_collects_late_artifacts_or_retries_execution() {
    let (result, steps) = fixture("timeout");
    assert_eq!(result.execution.status, "timed_out");
    assert_eq!(result.evidence["termination_confirmed"], true);
    assert!(result.artifact.is_none());
    assert!(!steps.iter().any(|s| s == "candidate"));
    assert_eq!(steps.iter().filter(|s| s.as_str() == "execute").count(), 1);
}
#[test]
fn cancellation_after_capture_discards_candidate_and_does_not_cancel_cleanup() {
    let (result, _) = fixture("cancel");
    assert_eq!(result.execution.status, "cancelled");
    assert_eq!(result.evidence["termination_confirmed"], true);
    assert!(result.artifact.is_none());
}
#[test]
fn unconfirmed_or_replaced_sandbox_cannot_publish_success() {
    for failure in ["stop", "identity", "generation"] {
        let (result, _) = fixture(failure);
        assert_eq!(result.execution.status, "termination_unconfirmed");
        assert!(result.artifact.is_none());
    }
}
#[test]
fn foreign_owner_is_not_stopped_or_published() {
    let (result, steps) = fixture("owner");
    assert_eq!(result.execution.status, "termination_unconfirmed");
    assert!(!steps.iter().any(|step| step == "stop"));
    assert!(result.artifact.is_none());
}
#[test]
fn ambiguous_create_is_stopped_without_recreation() {
    let (result, steps) = fixture("create");
    assert_eq!(result.execution.status, "failed");
    assert_eq!(steps.iter().filter(|s| s.as_str() == "create").count(), 1);
    assert!(!steps.iter().any(|s| s == "execute"));
    assert_eq!(result.evidence["termination_confirmed"], true);
}
#[test]
fn over_limit_candidate_is_rejected_and_sandbox_stopped() {
    let (result, _) = fixture("oversize");
    assert_eq!(result.execution.status, "failed");
    assert!(result.artifact.is_none());
    assert_eq!(result.evidence["termination_confirmed"], true);
}
