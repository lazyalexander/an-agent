//! Evidence gate for post-run host actions, not remote execution attestation.
use crate::{
    execution::{Result, store},
    file_job::regular,
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const LIMIT: usize = 65536;

pub(crate) struct Accepted {
    pub directory: PathBuf,
    pub artifact: Vec<u8>,
    pub evidence: Value,
}

fn native_file(receipt: &Value) -> bool {
    matches!(
        receipt["backend"].as_str(),
        Some("macos-file-kit-v1" | "linux-landlock-seccomp-file-v1")
    )
}

pub(crate) fn seal(run: &Path, receipt: &Value) -> Result<()> {
    // Do not impose a new metadata-size/retention contract on other backends.
    if !native_file(receipt) {
        return Ok(());
    }
    let marker = json!({"schema":"an-host-settlement-v1",
        "run_id":receipt["run_id"],"operation":receipt["operation"]["intent"],
        "receipt_sha256":store::digest(&regular(&run.join("receipt.json"), LIMIT)?),
        "audit_sha256":store::digest(&store::read(&run.join("audit.json"), 1024 * 1024)?)});
    store::write_new(
        &run.join("settled.json"),
        &serde_json::to_vec_pretty(&marker)?,
    )
}

/// Call while holding the whole-store lock. Host storage is trusted and must
/// never be writable by a kit or an untrusted same-user process.
pub(crate) fn accepted(root: &Path, id: &str) -> Result<Accepted> {
    inspect(root, id, false)
}

/// Cleanup may also inspect a cancelled direct child, but never authorize a
/// capability or accept its partial output. Uncertain termination stays fenced.
pub(crate) fn reclaimable(root: &Path, id: &str) -> Result<Accepted> {
    inspect(root, id, true)
}

fn inspect(root: &Path, id: &str, allow_cancelled: bool) -> Result<Accepted> {
    if uuid::Uuid::parse_str(id)?.to_string() != id {
        return Err("run must be a canonical UUID".into());
    }
    store::no_links(root)?;
    let directory = root.canonicalize()?.join("runs").join(id);
    let receipt_bytes = regular(&directory.join("receipt.json"), LIMIT)?;
    let receipt: Value = serde_json::from_slice(&receipt_bytes)?;
    let marker: Value = serde_json::from_slice(&regular(&directory.join("settled.json"), LIMIT)?)?;
    let operation: Value =
        serde_json::from_slice(&regular(&directory.join("operation.json"), LIMIT)?)?;
    let observation = &receipt["operation"]["observation"];
    let plan_bytes = regular(&directory.join("plan.json"), LIMIT)?;
    let plan: Value = serde_json::from_slice(&plan_bytes)?;
    let backend = receipt["backend"].as_str().unwrap_or_default();
    let cancelled = allow_cancelled
        && receipt["status"] == "cancelled"
        && observation["status"] == "cancelled"
        && receipt["operation"]["cancel_requested"] == true
        && receipt["artifact"].is_null()
        && !directory.join("artifact.txt").exists();
    let successful = receipt["status"] == "succeeded"
        && receipt["exit_code"] == 0
        && observation["status"] == "succeeded"
        && observation["exit_code"] == 0
        && receipt["artifact"]["path"] == "artifact.txt";
    if receipt["schema_version"] != 1
        || receipt["run_id"] != id
        || !(successful || cancelled)
        || receipt["process"]["reaped"] != true
        || receipt["process"]["scope"] != "direct-child"
        || !native_file(&receipt)
        || observation["termination_confirmed"] != true
        || observation.get("reconciliation_error").is_some()
        || operation["run_id"] != id
        || receipt["operation"]["intent"] != operation
        || observation["operation"] != operation
        || marker["schema"] != "an-host-settlement-v1"
        || marker["run_id"] != id
        || marker["operation"] != operation
        || marker["receipt_sha256"] != store::digest(&receipt_bytes)
        || marker["audit_sha256"]
            != store::digest(&store::read(&directory.join("audit.json"), 1024 * 1024)?)
        || receipt["plan_sha256"] != store::digest(&plan_bytes)
        || plan["backend"] != backend
        || plan["release"] != receipt["release"]
        || plan["files"]["other_writes"] != false
        || plan["cwd"] != json!(directory.join("work"))
    {
        return Err(
            "post-run action requires a settled, termination-confirmed native file run".into(),
        );
    }
    uuid::Uuid::parse_str(
        operation["operation_id"]
            .as_str()
            .ok_or("missing operation identity")?,
    )?;
    let artifact = if cancelled {
        Vec::new()
    } else {
        regular(&directory.join("artifact.txt"), LIMIT)?
    };
    if !cancelled
        && (receipt["artifact"]["sha256"] != store::digest(&artifact)
            || receipt["artifact"]["bytes"].as_u64() != Some(artifact.len() as u64))
    {
        return Err("accepted artifact identity mismatch".into());
    }
    Ok(Accepted {
        directory,
        artifact,
        evidence: json!({"run_id":id,
        "receipt_sha256":store::digest(&receipt_bytes),"settlement":marker,
        "artifact":receipt["artifact"],"backend":backend}),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_profiles_do_not_acquire_a_retention_contract() -> Result<()> {
        let temp = an_agent_core::testkit::TempDir::new("no-retention-marker");
        for backend in ["openshell-vm-file-v1", "macos-seatbelt-stdio-v1", "unknown"] {
            // No receipt/audit files: unrelated backends must not even read them.
            seal(temp.path(), &json!({"backend":backend}))?;
            assert!(!temp.path().join("settled.json").exists());
        }
        Ok(())
    }
}
