//! Bounded space observation and collection of private snapshots, never products.
use super::*;

fn logical_bytes(path: &Path, entries: &mut usize, depth: usize) -> Result<u64> {
    if depth > 64 || *entries >= 250_000 {
        return Err("storage observation exceeds its bounded inventory".into());
    }
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error.into()),
    };
    *entries += 1;
    if meta.is_symlink() {
        return Ok(0);
    }
    if meta.is_file() {
        return Ok(meta.len());
    }
    let mut bytes = 0u64;
    if meta.is_dir() {
        for child in fs::read_dir(path)? {
            bytes = bytes
                .checked_add(logical_bytes(&child?.path(), entries, depth + 1)?)
                .ok_or("storage count overflow")?;
        }
    }
    Ok(bytes)
}

fn size(path: &Path) -> Result<u64> {
    logical_bytes(path, &mut 0, 0)
}

/// No lock, no hash-based authority: concurrent writes may change this observation.
pub fn storage(path: &Path) -> Result<Value> {
    let root = location(path)?;
    let (instance, _) = state(&root)?;
    let mut runs = Vec::new();
    for entry in fs::read_dir(root.join("runs"))? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let directory = entry.path();
        let name = directory
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("run name")?;
        let status = status::status(&root, name)?;
        let snapshots = directory.join("job/attempts");
        let mut inputs = 0u64;
        let mut collected = false;
        let mut receipt_present = false;
        if snapshots.exists() {
            store::no_links(&snapshots)?;
            for attempt in fs::read_dir(&snapshots)? {
                let attempt = attempt?;
                if attempt.file_type()?.is_dir() {
                    inputs = inputs
                        .checked_add(size(&attempt.path().join("snapshot/inputs"))?)
                        .ok_or("snapshot count overflow")?;
                    collected |= attempt.path().join("snapshot-reclaim-intent.json").exists();
                    receipt_present |= attempt.path().join("snapshot-reclaimed.json").exists();
                }
            }
        }
        runs.push(json!({"run":name,"work_logical_bytes":size(&directory.join("job/work"))?,
            "snapshot_logical_bytes":inputs,"artifact_logical_bytes":size(&directory.join("job/artifacts"))?,
            "termination_confirmed":status["termination_confirmed"],
            "snapshot_collection_intent_present":collected,"snapshot_collection_receipt_present":receipt_present,
            "snapshot_cleanup_candidate":!collected && directory.join("reclaimed.json").exists()
                && status["termination_confirmed"] == true && inputs > 0}));
    }
    runs.sort_by_key(|r| r["run"].as_str().unwrap_or_default().to_owned());
    Ok(
        json!({"schema":"an-native-spool-storage-v1","instance":instance["instance"],
        "release":instance["release"],"total_logical_bytes":size(&root)?,
        "release_logical_bytes":size(&root.join("release"))?,"kit_logical_bytes":size(&root.join("kit"))?,
        "state_logical_bytes":size(&root.join("checkpoints"))?,"runs":runs,
        "filesystem_available_bytes":crate::workspace_job::capacity::available(&root)?,
        "retained_roots":["release","kit","committed checkpoints","formal artifacts","execution and control evidence"],
        "authority":"observation only; cleanup revalidates under the owning locks",
        "accounting":"logical file lengths; APFS shared blocks are not independently reclaimable"}),
    )
}

pub fn reclaim_snapshots(path: &Path, name: &str, approved: Option<&str>) -> Result<Value> {
    let root = location(path)?;
    let _lock = store::lock(&root)?;
    let result = run::settled(&root, name)?;
    let directory = root.join("runs").join(name);
    let job = directory.join("job");
    let work: Value = serde_json::from_slice(&regular(&job.join("reclaimed.json"), 65536)?)?;
    let outer: Value = serde_json::from_slice(&regular(&directory.join("reclaimed.json"), 65536)?)?;
    if outer
        != json!({"status":"succeeded","instance":result["instance"],"release":result["release"],
        "run":name,"operation":"reclaim","result":work})
    {
        return Err("snapshot collection requires instance-bound work collection".into());
    }
    if result["status"] == "succeeded" {
        artifacts::verified(&root, name, true)?;
    }
    if root.join("checkpoints").exists() {
        checkpoint::head_selection(&root)?;
    }
    let id = result["workspace"]["attempt"].as_str().ok_or("attempt")?;
    let preview = crate::workspace_job::reclaim_snapshots_managed(&job, id, None)?;
    if preview.get("plan").is_none() {
        return Ok(
            json!({"status":preview["status"],"instance":result["instance"],
            "run":name,"result":preview,"executed":false}),
        );
    }
    let plan = json!({"schema":"an-native-spool-snapshot-reclaim-v1","instance":result["instance"],
        "release":result["release"],"run":name,"workspace":preview["plan"]});
    let (hash, execute) = approval(&plan, approved)?;
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":hash,"executed":false}));
    }
    let result = crate::workspace_job::reclaim_snapshots_managed(
        &job,
        id,
        preview["approval_sha256"].as_str(),
    )?;
    Ok(json!({"status":result["status"],"run":name,"result":result}))
}
