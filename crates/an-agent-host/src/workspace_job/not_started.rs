//! Positive host evidence of no spawn, never inferred from a missing process ID.
use crate::{
    execution::{Result, sandbox, store},
    file_job::regular,
};
use serde_json::{Value, json};
use std::path::Path;

pub(super) enum Boundary {
    IntentPersisted,
    ReadyToLaunch,
}

pub(super) fn cancelled_before_spawn(result: &sandbox::Execution) -> bool {
    result.status == "cancelled"
        && result.pid.is_none()
        && !result.reaped
        && result.code.is_none()
        && !result.group_cleanup_required
        && result.stdout.is_empty()
        && result.stderr.is_empty()
}

pub(super) fn finish(
    dir: &Path,
    id: &str,
    plan: &[u8],
    stage: &str,
    cancelled: bool,
    error: Option<&(dyn std::error::Error + 'static)>,
) -> Result<Value> {
    let intent = regular(&dir.join("intent.json"), 65536)?;
    let mut receipt = json!({"schema":"an-workspace-not-started-v1",
        "attempt":id,"plan_sha256":store::digest(plan),"intent_sha256":store::digest(&intent),
        "status":if cancelled {"cancelled"} else {"failed"},"stage":stage,
        "process_started":false,"pid":null,"exit_code":null,"reaped":false,
        "termination_confirmed":true,"group_cleanup_required":false,"automatic_retries":0});
    if let Some(error) = error {
        receipt["error"] = sandbox::refusal_evidence(error);
    }
    store::write_new(
        &dir.join("receipt.json"),
        &serde_json::to_vec_pretty(&receipt)?,
    )?;
    Ok(receipt)
}

pub(super) fn verified(dir: &Path, receipt: &Value) -> Result<bool> {
    let stage_valid = match receipt["stage"].as_str() {
        Some("preparation") => receipt["error"].is_object(),
        Some("supervisor") => receipt["status"] == "cancelled" && receipt.get("error").is_none(),
        _ => false,
    };
    if !stage_valid
        || !matches!(receipt["status"].as_str(), Some("failed" | "cancelled"))
        || receipt["process_started"] != false
        || receipt["reaped"] != false
        || receipt.get("pid") != Some(&Value::Null)
        || receipt.get("exit_code") != Some(&Value::Null)
        || receipt["termination_confirmed"] != true
        || receipt["group_cleanup_required"] != false
        || receipt.get("group_observation").is_some()
    {
        return Ok(false);
    }
    let bytes = regular(&dir.join("intent.json"), 65536)?;
    let intent: Value = serde_json::from_slice(&bytes)?;
    Ok(
        receipt["intent_sha256"] == store::digest(&bytes)
            && intent["attempt"] == receipt["attempt"],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_supervisors_exact_prelaunch_result_is_no_spawn_evidence() {
        let mut result = sandbox::Execution {
            status: "cancelled",
            code: None,
            stdout: vec![],
            stderr: vec![],
            pid: None,
            reaped: false,
            group_cleanup_required: false,
        };
        assert!(cancelled_before_spawn(&result));
        result.status = "failed";
        assert!(!cancelled_before_spawn(&result));
        result.status = "cancelled";
        result.pid = Some(123);
        assert!(!cancelled_before_spawn(&result));
        result.pid = None;
        result.reaped = true;
        assert!(!cancelled_before_spawn(&result));
        result.reaped = false;
        result.stdout = b"a process wrote this".to_vec();
        assert!(!cancelled_before_spawn(&result));
    }

    #[test]
    fn not_started_gate_requires_durable_intent_and_explicit_fields() -> Result<()> {
        let temp = an_agent_core::testkit::TempDir::new("not-started-gate");
        let dir = temp.path().canonicalize()?;
        let intent = serde_json::to_vec(&json!({"attempt":"attempt"}))?;
        store::write_new(&dir.join("intent.json"), &intent)?;
        let receipt = finish(&dir, "attempt", b"plan", "supervisor", true, None)?;
        assert!(verified(&dir, &receipt)?);
        for field in ["pid", "reaped", "process_started", "stage", "intent_sha256"] {
            let mut missing = receipt.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(!verified(&dir, &missing)?);
        }
        std::fs::write(dir.join("intent.json"), b"{}")?;
        assert!(!verified(&dir, &receipt)?);
        Ok(())
    }
}
