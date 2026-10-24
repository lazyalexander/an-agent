//! Formal output publication is separate from a runtime's private recovery files.
use super::{approval, inventory, record, root};
use crate::{
    execution::{Result, store},
    file_job::regular,
};
use serde_json::{Value, json};
use std::path::Path;

pub fn accept(path: &Path, id: &str, approved: Option<&str>) -> Result<Value> {
    let root = root(path)?;
    let _lock = store::lock(&root)?;
    let (dir, receipt) = record(&root, id)?;
    if receipt["status"] != "succeeded" || receipt["exit_code"] != 0 {
        return Err("only successful terminated attempts may publish outputs".into());
    }
    let execution: Value =
        serde_json::from_slice(&regular(&dir.join("plan.json"), 2 * 1024 * 1024)?)?;
    super::profile::verify_inputs(&execution)?;
    let mut candidates = Vec::new();
    let mut payloads = Vec::new();
    let mut total = 0usize;
    for name in execution["outputs"].as_array().ok_or("missing outputs")? {
        let name = name.as_str().ok_or("invalid output name")?;
        inventory::relative(name)?;
        let binary = execution["binary_outputs"]
            .as_array()
            .is_some_and(|names| names.iter().any(|n| n == name));
        let bytes = regular(
            &root.join("work").join(name),
            if binary {
                crate::artifact_versions::BINARY_LIMIT
            } else {
                65536
            },
        )?;
        if !binary {
            std::str::from_utf8(&bytes)?;
        }
        total += bytes.len();
        if total > 16 * 1024 * 1024 {
            return Err("candidate outputs exceed 16 MiB".into());
        }
        let mut candidate = json!({"path":name,"sha256":store::digest(&bytes),"bytes":bytes.len()});
        if binary {
            candidate["encoding"] = json!("bytes");
        }
        candidates.push(candidate);
        payloads.push(bytes);
    }
    let plan = json!({"schema":"an-workspace-accept-v1","attempt":id,
        "receipt_sha256":store::digest(&regular(&dir.join("receipt.json"), 2 * 1024 * 1024)?),
        "outputs":candidates});
    let (digest, execute) = approval(&plan, approved)?;
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":digest,"executed":false}));
    }
    store::write_new(&dir.join("accept-intent.json"), &serde_json::to_vec(&plan)?)?;
    let mut revisions = Vec::new();
    for (item, bytes) in candidates.iter().zip(&payloads) {
        let mut source = json!({"kind":"workspace-output","attempt":id,"path":item["path"],
            "receipt_sha256":plan["receipt_sha256"],"profile_sha256":execution["profile_sha256"]});
        if let Some(inputs) = execution["artifact_inputs"].as_array() {
            source["input_revisions"] = json!(
                inputs
                    .iter()
                    .map(|i| json!({"revision":i["binding"]["revision"],"content_sha256":i["binding"]["content_sha256"],"bytes":i["binding"]["bytes"]}))
                    .collect::<Vec<_>>()
            );
        }
        let revision = if item["encoding"] == "bytes" {
            crate::artifact_versions::publish_bytes(&root.join("artifacts"), bytes, source)?
        } else {
            crate::artifact_versions::publish(&root.join("artifacts"), bytes, None, source)?
        };
        revisions.push(json!({"path":item["path"],"binding":revision}));
    }
    let result = json!({"status":"succeeded","schema":"an-workspace-accepted-v1","approval_sha256":digest,"outputs":revisions});
    store::write_new(
        &dir.join("accepted.json"),
        &serde_json::to_vec_pretty(&result)?,
    )?;
    Ok(result)
}

/// Explicitly abandons resume rights; never selects formal artifacts for deletion.
pub fn reclaim(path: &Path, id: &str, approved: Option<&str>) -> Result<Value> {
    let root = root(path)?;
    let _lock = store::lock(&root)?;
    let (dir, receipt) = record(&root, id)?;
    if receipt["status"] == "succeeded" {
        let accepted: Value = serde_json::from_slice(&regular(&dir.join("accepted.json"), 65536)?)?;
        if accepted["status"] != "succeeded" || accepted["schema"] != "an-workspace-accepted-v1" {
            return Err("successful outputs must be accepted before scratch collection".into());
        }
        for output in accepted["outputs"]
            .as_array()
            .ok_or("missing accepted bindings")?
        {
            crate::artifact_versions::read(
                &root.join("artifacts"),
                output["binding"]["revision"]
                    .as_str()
                    .ok_or("missing revision")?,
            )?;
        }
    }
    let inventory = inventory::scan(&root.join("work"))?;
    let plan = json!({"schema":"an-workspace-reclaim-v1","attempt":id,
        "receipt_sha256":store::digest(&regular(&dir.join("receipt.json"), 2 * 1024 * 1024)?),
        "work":root.join("work"),"inventory":inventory,"abandon_resume":true,
        "preserve":"accepted artifact catalog, plans, receipts and diagnostic streams"});
    let (digest, execute) = approval(&plan, approved)?;
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":digest,"executed":false}));
    }
    store::write_new(
        &root.join("reclaim-intent.json"),
        &serde_json::to_vec(&plan)?,
    )?;
    inventory::remove(&root.join("work"), &inventory)?;
    let result = json!({"status":"succeeded","approval_sha256":digest,"resume_available":false});
    store::write_new(
        &root.join("reclaimed.json"),
        &serde_json::to_vec_pretty(&result)?,
    )?;
    Ok(result)
}
