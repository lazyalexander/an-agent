use super::*;

pub(super) fn outcome() -> Outcome {
    Outcome {
        operation: Operation {
            run_id: "run".into(),
            operation_id: "attempt".into(),
        },
        execution: sandbox::Execution {
            status: "succeeded",
            code: Some(0),
            stdout: vec![],
            stderr: vec![],
            pid: Some(7),
            reaped: true,
        },
        environment: Some(Environment {
            id: "sandbox".into(),
            generation: "generation-1".into(),
        }),
        settled_environment: Some(Environment {
            id: "sandbox".into(),
            generation: "generation-1".into(),
        }),
        termination_confirmed: true,
        requires_environment: true,
        candidate: Candidate::Bytes(b"product".to_vec()),
        evidence: Value::Null,
    }
}

#[test]
fn success_requires_the_same_operation_environment_and_generation() {
    let mut result = outcome();
    let expected = result.operation.clone();
    assert!(result.reconcile(&expected).is_ok());
    result.operation.operation_id = "old-attempt".into();
    assert!(result.reconcile(&expected).is_err());
    result.operation = expected.clone();
    result.operation.run_id = "another-run".into();
    assert!(result.reconcile(&expected).is_err());
    result.operation = expected.clone();
    result
        .settled_environment
        .as_mut()
        .expect("test fixture")
        .generation = "generation-2".into();
    assert!(result.reconcile(&expected).is_err());
    result.settled_environment = result.environment.clone();
    result
        .settled_environment
        .as_mut()
        .expect("test fixture")
        .id = "replacement".into();
    assert!(result.reconcile(&expected).is_err());
    result.environment = None;
    result.settled_environment = None;
    assert!(result.reconcile(&expected).is_err());
}

#[test]
fn termination_is_not_inferred_from_a_successful_transport_process() {
    let mut result = outcome();
    result.termination_confirmed = false;
    assert!(result.execution.reaped);
    assert!(result.reconcile(&result.operation).is_err());
    result.execution.status = "termination_unconfirmed";
    assert!(
        result.reconcile(&result.operation).is_ok(),
        "retain failure evidence"
    );
}

#[test]
fn native_does_not_invent_a_vm_generation() {
    let mut result = outcome();
    result.requires_environment = false;
    result.environment = None;
    result.settled_environment = None;
    assert!(result.reconcile(&result.operation).is_ok());
}

#[test]
fn bounded_candidates_are_data_not_host_paths() {
    let path = Path::new("/unused");
    assert!(accept(Candidate::None, path).is_err());
    assert!(accept(Candidate::Bytes(vec![b'x'; 65537]), path).is_err());
    assert!(accept(Candidate::Bytes(vec![255]), path).is_err());
    assert_eq!(
        accept(Candidate::Bytes(b"data".to_vec()), path).expect("test fixture"),
        b"data"
    );
}
