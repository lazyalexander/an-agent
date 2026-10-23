//! Fault injection crosses the real registration/admission/receipt boundary.
use super::*;
use crate::execution::{register, run_backend, store};
use an_agent_core::testkit::TempDir;
use serde_json::json;
use std::{fs, sync::atomic::Ordering};

struct Fault(&'static str);
impl ExecutionBackend for Fault {
    fn validate(&self, file: bool) -> Result<()> {
        assert!(file);
        Ok(())
    }
    fn plan(&self, _: &Path, _: &Path, _: bool, plan: &mut Value) -> Result<()> {
        plan["backend"] = json!("test-evidence");
        Ok(())
    }
    fn execute(&self, request: Request<'_>, _: bool) -> Result<Outcome> {
        // Submission cannot precede durable operation intent.
        let intent: Value =
            serde_json::from_slice(&fs::read(request.directory.join("operation.json"))?)?;
        assert_eq!(intent, serde_json::to_value(request.operation)?);
        let mut result = super::tests::outcome();
        result.operation = request.operation.clone();
        result.execution.stdout = b"retained diagnostics".to_vec();
        match self.0 {
            "operation" => result.operation.operation_id = "stale".into(),
            "generation" => {
                result
                    .settled_environment
                    .as_mut()
                    .ok_or("missing test environment")?
                    .generation = "stale".into()
            }
            "unknown" => {
                result.termination_confirmed = false;
                result.execution.status = "termination_unconfirmed";
            }
            "cancel" => request.cancelled.store(true, Ordering::Relaxed),
            _ => (),
        }
        Ok(result)
    }
    fn cancel_before_acceptance(&self) -> bool {
        true
    }
    fn decorate_receipt(&self, _: &mut Value, _: Value, _: bool) {}
}

#[test]
fn host_rejects_stale_unknown_and_cancelled_results_without_losing_evidence() -> Result<()> {
    let temp = TempDir::new("backend-contract");
    let root = temp.path().canonicalize()?;
    let deployment = root.join("deployment");
    fs::create_dir(&deployment)?;
    let bytes = fs::read("/usr/bin/true")?;
    fs::write(deployment.join("program"), &bytes)?;
    fs::write(
        deployment.join("spool.yaml"),
        format!(
            "v: 1\nkind: spool\nname: boundary-test\nversion: 1.0.0\nsummary: Backend contract test\nsort: beat\nconstructor: host\nhost: an-agent-file-v1\neffect:\n  file: {{ op: rw, path: /work, recursive: true }}\n  memory: {{ op: ignore }}\n  net: none\n  proc: spawn\n  model: {{ op: none }}\n  flow: both\ninverse: irreversible\nrequires: []\nconsumes: [kit.request]\nproduces: [kit.execute]\nconfig:\n  profile: file-kit-v1\n  executable_sha256: \"{}\"\n",
            store::digest(&bytes)
        ),
    )?;
    let store = root.join("store");
    let registration = register(&deployment, &store)?;
    let release = registration["registered"].as_str().expect("test fixture");
    let input = root.join("input");
    fs::write(&input, b"data")?;
    for (fault, status) in [
        ("ok", "succeeded"),
        ("operation", "refused"),
        ("generation", "refused"),
        ("unknown", "termination_unconfirmed"),
        ("cancel", "cancelled"),
    ] {
        let result = run_backend(
            &store,
            release,
            &input,
            true,
            &AtomicBool::new(false),
            &Fault(fault),
        )?;
        assert_eq!(result["status"], status);
        let receipt_path = Path::new(result["receipt"].as_str().expect("test fixture"));
        let run = receipt_path.parent().expect("test fixture");
        let receipt: Value = serde_json::from_slice(&fs::read(receipt_path)?)?;
        assert_eq!(fs::read(run.join("stdout.txt"))?, b"retained diagnostics");
        assert_eq!(receipt["process"]["pid"], 7);
        assert_eq!(run.join("artifact.txt").exists(), fault == "ok");
        assert_eq!(!receipt["artifact"].is_null(), fault == "ok");
        assert!(run.join("audit.json").exists());
        if matches!(fault, "operation" | "generation") {
            assert!(receipt["operation"]["observation"]["reconciliation_error"].is_string());
        }
    }
    assert_eq!(
        fs::read_dir(store.join("runs"))?.count(),
        5,
        "no automatic retries"
    );
    Ok(())
}
