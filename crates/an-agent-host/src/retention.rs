//! Explicit reclamation of fixed native work copies after confirmed settlement.
use crate::{
    execution::{Result, store},
    file_job::regular,
    settled_run,
};
use serde_json::{Value, json};
use std::{fs, path::Path};

/// Preview by default. Approval binds evidence and exact work bytes. No recursive
/// deletion, failed-run cleanup, VM cleanup, stale-lock removal or automatic retry.
pub fn reclaim(root: &Path, run: &str, approved: Option<&str>) -> Result<Value> {
    store::no_links(root)?;
    let root = root.canonicalize()?;
    let _lock = store::lock(&root)?;
    let accepted = settled_run::accepted(&root, run)?;
    let directory = &accepted.directory;
    let work = directory.join("work");
    store::no_links(&work)?;
    let mut names = fs::read_dir(&work)?
        .map(|entry| entry.map(|e| e.file_name()))
        .collect::<std::io::Result<Vec<_>>>()?;
    names.sort();
    if names != ["input.txt", "output.txt"] {
        return Err("unexpected work contents; no reclamation performed".into());
    }
    let files = ["input.txt", "output.txt"].map(|name| {
        regular(&work.join(name), 65536)
            .map(|bytes| json!({"name":name,"sha256":store::digest(&bytes),"bytes":bytes.len()}))
    });
    let [input, output] = files;
    let plan = json!({"schema":"an-work-reclaim-v1","store":root,
        "target":format!("runs/{run}/work"),"evidence":accepted.evidence,
        "files":[input?,output?],"preserve":"all files outside this work directory"});
    let digest = store::digest(&serde_json::to_vec(&plan)?);
    if approved.is_none() {
        return Ok(json!({"plan":plan,"approval_sha256":digest,"executed":false}));
    }
    if approved != Some(digest.as_str()) {
        return Err("reclamation approval mismatch; inspect a fresh preview".into());
    }
    // Exclusive intent is also the duplicate/uncertain-attempt fence. A partial
    // deletion needs operator inspection, never an inferred retry authorization.
    store::write_new(
        &directory.join("reclaim-intent.json"),
        &serde_json::to_vec_pretty(&plan)?,
    )?;
    let result = (|| -> Result<()> {
        fs::remove_file(work.join("input.txt"))?;
        fs::remove_file(work.join("output.txt"))?;
        fs::remove_dir(&work)?;
        Ok(())
    })();
    let receipt = json!({"schema":"an-work-reclaim-result-v1","approval_sha256":digest,
        "status":if result.is_ok() {"succeeded"} else {"failed"},
        "error":result.as_ref().err().map(|e| e.to_string()),"automatic_retries":0});
    store::write_new(
        &directory.join("reclaim.json"),
        &serde_json::to_vec_pretty(&receipt)?,
    )?;
    result?;
    Ok(receipt)
}
