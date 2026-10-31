//! Finite private-material collection reuses per-run authority and deletion fences.
use super::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Material {
    Work,
    Snapshots,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    material: Material,
    runs: Vec<String>,
}

pub fn collect_private(
    path: &Path,
    request: &Path,
    approved: Option<&str>,
    cancelled: &AtomicBool,
) -> Result<Value> {
    let request: Request = serde_json::from_slice(&regular(request, 65536)?)?;
    if request.schema != "an-native-private-collection-request-v1"
        || !(1..=16).contains(&request.runs.len())
    {
        return Err("private collection requires 1..16 selected runs".into());
    }
    let mut seen = BTreeSet::new();
    for name in &request.runs {
        bundle::identifier(name)?;
        if !seen.insert(name) {
            return Err("duplicate collection run".into());
        }
    }
    let root = location(path)?;
    let (instance, _) = state(&root)?;
    let operation = match request.material {
        Material::Work => run::reclaim,
        Material::Snapshots => storage::reclaim_snapshots,
    };
    let mut entries = Vec::new();
    // Validate the whole selection before any deletion. Child previews own their locks.
    for name in &request.runs {
        if cancelled.load(Ordering::Acquire) {
            return Ok(
                json!({"status":"cancelled","reason":"stopped-before-deletion",
                "completed":[],"automatic_retries":0}),
            );
        }
        let preview = operation(&root, name, None)?;
        if !preview["plan"]["workspace"].is_object()
            || preview["plan"]["instance"] != instance["instance"]
            || preview["plan"]["release"] != instance["release"]
            || preview["plan"]["run"] != *name
            || !preview["approval_sha256"]
                .as_str()
                .is_some_and(store::valid_digest)
        {
            return Err(
                "selection is already collected, uncertain or changed; inspect per-run evidence"
                    .into(),
            );
        }
        entries.push(preview);
    }
    let plan = json!({"schema":"an-native-private-collection-v1","instance":instance["instance"],
        "release":instance["release"],"material":request.material,"runs":request.runs,"entries":entries,
        "preserve":"formal artifacts, checkpoint state, release/kit, evidence and external input files",
        "automatic_retries":0});
    let (digest, execute) = approval(&plan, approved)?;
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":digest,"executed":false}));
    }
    let mut completed = Vec::new();
    for (index, name) in request.runs.iter().enumerate() {
        if cancelled.load(Ordering::Acquire) {
            return Ok(
                json!({"status":"cancelled","reason":"stopped-between-items",
                "completed":completed,"automatic_retries":0}),
            );
        }
        // The original operation revalidates exact bytes/ownership and writes its own intent.
        let result = operation(&root, name, entries[index]["approval_sha256"].as_str());
        let result = match result {
            Ok(value) if value["status"] == "succeeded" => value,
            other => {
                return Ok(
                    json!({"status":"uncertain","reason":"collection-not-confirmed","run":name,
                "completed":completed,"automatic_retries":0,
                "diagnostic":match other {Ok(value)=>value.to_string(),Err(error)=>error.to_string()}}),
                );
            }
        };
        completed.push(json!({"run":name,"receipt":result}));
    }
    Ok(
        json!({"status":"succeeded","material":request.material,"completed":completed,"automatic_retries":0}),
    )
}
