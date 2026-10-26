//! Host custody for a reviewed local workflow's scratch and selected outputs.
mod custody;
mod inputs;
mod inventory;
mod not_started;
mod profile;
mod snapshot;

use crate::{
    execution::{Result, sandbox, store},
    file_job::regular,
};
pub use custody::{accept, reclaim};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

pub(super) fn root(path: &Path) -> Result<PathBuf> {
    store::no_links(path)?;
    if path.exists() {
        return Ok(path.canonicalize()?);
    }
    Ok(path
        .parent()
        .ok_or("missing job parent")?
        .canonicalize()?
        .join(path.file_name().ok_or("missing job name")?))
}

pub(super) fn attempt(root: &Path, id: &str) -> Result<PathBuf> {
    if uuid::Uuid::parse_str(id)?.to_string() != id {
        return Err("invalid attempt identity".into());
    }
    Ok(root.join("attempts").join(id))
}

pub(super) fn record(root: &Path, id: &str) -> Result<(PathBuf, Value)> {
    let dir = attempt(root, id)?;
    let bytes = regular(&dir.join("receipt.json"), 2 * 1024 * 1024)?;
    let receipt: Value = serde_json::from_slice(&bytes)?;
    let plan_bytes = regular(&dir.join("plan.json"), 2 * 1024 * 1024)?;
    let plan: Value = serde_json::from_slice(&plan_bytes)?;
    let settled = if receipt["schema"] == "an-workspace-not-started-v1" {
        not_started::verified(&dir, &receipt)?
    } else {
        receipt["schema"] == "an-workspace-receipt-v1"
            && receipt["termination_confirmed"] == true
            && receipt["reaped"] == true
            && receipt["pid"]
                .as_i64()
                .is_some_and(|id| id > 0 && id <= i32::MAX as i64)
            && receipt["group_observation"]["empty"] == true
            && receipt["group_observation"]["group"] == receipt["pid"]
            && receipt["group_observation"].get("error").is_none()
            && receipt["group_cleanup_required"].as_bool().is_some()
            && (receipt["status"] != "succeeded" || receipt["group_cleanup_required"] == false)
    };
    if !settled
        || receipt["attempt"] != id
        || receipt["plan_sha256"] != store::digest(&plan_bytes)
        || plan["root"] != json!(root)
        || dir.join("successor.json").exists()
        || root.join("reclaim-intent.json").exists()
    {
        return Err(
            "operation requires the current, termination-confirmed workspace attempt".into(),
        );
    }
    Ok((dir, receipt))
}

pub(super) fn approval(plan: &Value, approved: Option<&str>) -> Result<(String, bool)> {
    let digest = store::digest(&serde_json::to_vec(plan)?);
    if approved.is_some_and(|a| a != digest) {
        return Err("workspace approval mismatch; inspect a fresh preview".into());
    }
    Ok((digest, approved.is_some()))
}

/// Fresh attempt or explicit continuation of a settled predecessor. No retry.
pub fn run(
    profile_path: &Path,
    path: &Path,
    previous: Option<&str>,
    approved: Option<&str>,
    cancelled: &AtomicBool,
) -> Result<Value> {
    run_inner(profile_path, path, previous, approved, cancelled, |_, _| {})
}

// Private deterministic fault seam; callers cannot alter launch ordering.
fn run_inner(
    profile_path: &Path,
    path: &Path,
    previous: Option<&str>,
    approved: Option<&str>,
    cancelled: &AtomicBool,
    mut boundary: impl FnMut(not_started::Boundary, &Path),
) -> Result<Value> {
    let root = root(path)?;
    let _guard = if root.exists() {
        Some(store::lock(&root)?)
    } else {
        None
    };
    let predecessor = if let Some(id) = previous {
        let (dir, _) = record(&root, id)?;
        json!({"attempt":id,"receipt_sha256":store::digest(&regular(&dir.join("receipt.json"), 2 * 1024 * 1024)?)})
    } else {
        if root.exists() {
            return Err("existing workspace requires an explicit settled predecessor".into());
        }
        Value::Null
    };
    if profile_path.canonicalize()?.starts_with(&root) {
        return Err("operator profile must be outside the workspace job".into());
    }
    let bytes = regular(profile_path, 65536)?;
    let (_, plan) = profile::Profile::plan(&bytes, &root, &predecessor)?;
    let (digest, execute) = approval(&plan, approved)?;
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":digest,"executed":false}));
    }
    if !cfg!(target_os = "macos") {
        return Err("workspace execution is macOS-only; no fallback".into());
    }
    if cancelled.load(Ordering::Acquire) {
        return Ok(
            json!({"status":"cancelled","executed":false,"process_started":false,
            "termination_confirmed":true,"automatic_retries":0}),
        );
    }
    if !root.exists() {
        store::directory(&root)?;
    }
    let _fresh_guard = if _guard.is_none() {
        Some(store::lock(&root)?)
    } else {
        None
    };
    if root.join("attempts").exists() && previous.is_none() {
        return Err("concurrent workspace initialization".into());
    }
    store::directory(&root.join("attempts"))?;
    store::directory(&root.join("work"))?;
    store::directory(&root.join("work/tmp"))?;
    let id = an_agent_core::det_seam::Entropy::os().uuid_v4().to_string();
    let dir = attempt(&root, &id)?;
    store::directory(&dir)?;
    let plan_bytes = serde_json::to_vec_pretty(&plan)?;
    store::write_new(&dir.join("plan.json"), &plan_bytes)?;
    if let Some(prior) = previous {
        store::write_new(
            &attempt(&root, prior)?.join("successor.json"),
            &serde_json::to_vec(&json!({"attempt":id,"approval_sha256":digest}))?,
        )?;
    }
    // Intent precedes spawn. Missing/unconfirmed receipt fences resume and cleanup.
    store::write_new(
        &dir.join("intent.json"),
        &serde_json::to_vec(&json!({"attempt":id,"approval_sha256":digest}))?,
    )?;
    boundary(not_started::Boundary::IntentPersisted, &dir);
    let prepared = snapshot::prepare(&plan, &dir, cancelled).and_then(|execution| {
        let bytes = regular(&dir.join("execution.json"), 2 * 1024 * 1024)?;
        Ok((execution, bytes))
    });
    let (execution_plan, execution_bytes) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            return not_started::finish(
                &dir,
                &id,
                &plan_bytes,
                "preparation",
                cancelled.load(Ordering::Acquire),
                Some(error.as_ref()),
            );
        }
    };
    boundary(not_started::Boundary::ReadyToLaunch, &dir);
    let execution = sandbox::workspace::execute(&execution_plan, cancelled);
    let mut receipt = json!({"schema":"an-workspace-receipt-v1","attempt":id,"plan_sha256":store::digest(&plan_bytes),
        "execution_sha256":store::digest(&execution_bytes),"status":"refused","reaped":false,"termination_confirmed":false,"automatic_retries":0});
    match execution {
        Ok(result) if not_started::cancelled_before_spawn(&result) => {
            return not_started::finish(&dir, &id, &plan_bytes, "supervisor", true, None);
        }
        Ok(result) => {
            let group_observation = sandbox::workspace::settlement(result.pid, result.reaped);
            let confirmed = result.reaped && group_observation["empty"] == true;
            receipt["group_observation"] = group_observation;
            receipt["group_cleanup_required"] = json!(result.group_cleanup_required);
            receipt["execution_status"] = json!(result.status);
            receipt["status"] = json!(if confirmed {
                result.status
            } else {
                "unconfirmed"
            });
            receipt["exit_code"] = json!(result.code);
            receipt["pid"] = json!(result.pid);
            receipt["reaped"] = json!(result.reaped);
            receipt["termination_confirmed"] = json!(confirmed);
            receipt["scope"] =
                json!("trusted-process-group; no detached or remote workers permitted");
            store::write_new(&dir.join("stdout.txt"), &result.stdout)?;
            store::write_new(&dir.join("stderr.txt"), &result.stderr)?;
            if confirmed {
                match inventory::scan(&root.join("work")) {
                    Ok(inventory) => receipt["scratch_after"] = inventory,
                    Err(error) => receipt["inventory_error"] = json!(error.to_string()),
                }
            }
        }
        Err(error) => {
            receipt["error"] = sandbox::refusal_evidence(error.as_ref());
        }
    }
    store::write_new(
        &dir.join("receipt.json"),
        &serde_json::to_vec_pretty(&receipt)?,
    )?;
    Ok(receipt)
}

#[cfg(all(test, target_os = "macos"))]
#[path = "prelaunch_tests.rs"]
mod prelaunch_tests;
