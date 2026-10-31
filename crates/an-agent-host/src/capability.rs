//! One explicit host-mediated artifact read; no general tool dispatch or RPC.
use crate::{
    artifact_versions,
    execution::{self, Result, store},
    settled_run,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    capability: String,
    revision: String,
}

/// The trusted caller chooses catalog and continuation, then approves the exact
/// preview digest. Request bytes are untrusted data, never host authority.
pub fn artifact_read(
    root: &Path,
    request_run: &str,
    catalog: &Path,
    continuation: &str,
    approved: Option<&str>,
    cancelled: &AtomicBool,
) -> Result<Value> {
    store::no_links(root)?;
    store::no_links(catalog)?;
    let root = root.canonicalize()?;
    let catalog = catalog.canonicalize()?;
    let accepted = {
        let _lock = store::lock(&root)?;
        settled_run::accepted(&root, request_run)?
    };
    let request: Request = serde_json::from_slice(&accepted.artifact)?;
    if request.schema != "an-capability-request-v1" || request.capability != "artifact.read" {
        return Err("unsupported external capability request".into());
    }
    // Reuse admission, immutable release checks and bounded revision validation.
    // This preview does not create a run or disclose catalog bytes to the kit.
    let execution_plan = execution::run_revision(
        &root,
        continuation,
        &catalog,
        &request.revision,
        false,
        cancelled,
    )?;
    if execution_plan["supported"] != true || execution_plan["files"]["other_writes"] != false {
        return Err("capability continuation requires a supported native file kit".into());
    }
    let plan = json!({"schema":"an-artifact-read-grant-v1","store":root,
        "request":accepted.evidence,"catalog":catalog,"revision":request.revision,
        "input_revision":execution_plan["input_revision"],"continuation_release":continuation,
        "backend":execution_plan["backend"],"limits":execution_plan["limits"],
        "policy":"fixed-native-file-kit-v1; selected bytes only; no catalog access",
        "automatic_retries":0});
    let digest = store::digest(&serde_json::to_vec(&plan)?);
    if approved.is_none() {
        return Ok(json!({"plan":plan,"approval_sha256":digest,"executed":false}));
    }
    if approved != Some(digest.as_str()) {
        return Err("external capability requires the exact host-approved grant digest".into());
    }
    let journal = root.join("capabilities").join(&digest);
    {
        let _lock = store::lock(&root)?;
        if settled_run::accepted(&root, request_run)?.evidence != accepted.evidence {
            return Err("request evidence changed since approval".into());
        }
        let (_, binding) = artifact_versions::admission::read_input(&catalog, &request.revision)?;
        if binding != execution_plan["input_revision"] {
            return Err("revision changed since approval".into());
        }
        if cancelled.load(Ordering::Relaxed) {
            return Err("capability cancelled before submission".into());
        }
        store::directory(&root.join("capabilities"))?;
        store::no_links(&journal)?;
        // Atomic directory creation fences duplicate and uncertain submissions.
        // A missing result after a crash never authorizes an automatic retry.
        std::fs::create_dir(&journal)?;
        store::write_new(
            &journal.join("intent.json"),
            &serde_json::to_vec_pretty(&plan)?,
        )?;
    }
    let result = execution::run_revision(
        &root,
        continuation,
        &catalog,
        &request.revision,
        true,
        cancelled,
    );
    let receipt = match &result {
        Ok(value) => json!({"schema":"an-capability-result-v1","approval_sha256":digest,
            "request_run":request_run,"status":value["status"],"continuation":value,
            "automatic_retries":0}),
        Err(error) => json!({"schema":"an-capability-result-v1","approval_sha256":digest,
            "request_run":request_run,"status":"uncertain","error":error.to_string(),
            "automatic_retries":0}),
    };
    store::write_new(
        &journal.join("result.json"),
        &serde_json::to_vec_pretty(&receipt)?,
    )?;
    result?;
    Ok(receipt)
}
