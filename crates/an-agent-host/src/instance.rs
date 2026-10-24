//! Bounded instance state publication over existing file-kit runs and revisions.
pub mod checkpoint;
mod storage;
use crate::{
    artifact_versions,
    execution::{self, Result, store},
    file_job::regular,
    settled_run,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{fs, path::Path, sync::atomic::AtomicBool};
use storage::{catalog, head, instance_path, read_bundle, release, root_lock};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Bundle {
    schema: String,
    state: String,
    product: String,
    report: String,
}

impl Bundle {
    fn parse(bytes: &[u8]) -> Result<Self> {
        let value: Self = serde_json::from_slice(bytes)?;
        if value.schema != "an-instance-bundle-v1" {
            return Err("unsupported instance bundle".into());
        }
        Ok(value)
    }
}

fn approval(plan: &Value, approved: Option<&str>) -> Result<(String, bool)> {
    let digest = store::digest(&serde_json::to_vec(plan)?);
    if let Some(value) = approved
        && value != digest
    {
        return Err("instance approval mismatch".into());
    }
    Ok((digest, approved.is_some()))
}

/// Initialize an explicit instance identity; no execution or implicit state migration.
pub fn init(
    root: &Path,
    id: &str,
    package: &str,
    state: &Path,
    approved: Option<&str>,
) -> Result<Value> {
    let (root, _lock) = root_lock(root)?;
    let path = instance_path(&root, id)?;
    if path.exists() {
        return Err("instance already exists".into());
    }
    storage::check_release(&root, package)?;
    let bundle = Bundle {
        schema: "an-instance-bundle-v1".into(),
        state: String::from_utf8(regular(state, 8192)?)?,
        product: String::new(),
        report: String::new(),
    };
    let bytes = serde_json::to_vec(&bundle)?;
    if bytes.len() > 8192 {
        return Err("initial bundle exceeds execution input limit".into());
    }
    let plan = json!({"schema":"an-instance-init-v1","store":root,"instance":id,
        "release":package,"bundle_sha256":store::digest(&bytes)});
    let (digest, execute) = approval(&plan, approved)?;
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":digest,"executed":false}));
    }
    store::directory(&root.join("instances"))?;
    let staging = root.join("instances").join(format!("{id}.pending"));
    // A previous interrupted initialization must be inspected, not overwritten.
    fs::create_dir(&staging)?;
    store::write_new(
        &staging.join("instance.json"),
        &serde_json::to_vec(&json!({
        "schema":"an-instance-v1","instance":id,"release":package}))?,
    )?;
    let first = artifact_versions::publish(
        &catalog(&staging),
        &bytes,
        None,
        json!({"kind":"instance-init","instance":id,"approval_sha256":digest}),
    )?;
    let revision = first["revision"]
        .as_str()
        .ok_or("missing initial revision")?;
    store::write_new(&staging.join("HEAD"), revision.as_bytes())?;
    fs::rename(staging, &path)?;
    Ok(json!({"status":"succeeded","instance":id,"revision":revision}))
}

/// Read only a committed revision (or current head), never an orphan candidate.
pub fn inspect(root: &Path, id: &str, revision: Option<&str>) -> Result<Value> {
    let (root, _lock) = root_lock(root)?;
    let path = instance_path(&root, id)?;
    let package = release(&path, id)?;
    let current = head(&path)?;
    let revision = revision.unwrap_or(&current);
    storage::committed(&path, &current, revision)?;
    let (bundle, binding) = read_bundle(&path, revision)?;
    Ok(
        json!({"instance":id,"release":package,"head":current,"revision":revision,
        "bundle":bundle,"binding":binding}),
    )
}

/// Execute from an exact committed state, using a fresh native run directory.
/// This never changes HEAD. An older committed input is allowed for evaluation;
/// publication will conflict if that version is no longer current.
pub fn run(
    root: &Path,
    id: &str,
    expected: &str,
    execute: bool,
    cancelled: &AtomicBool,
) -> Result<Value> {
    let (root, package, catalog) = {
        let (root, _lock) = root_lock(root)?;
        let path = instance_path(&root, id)?;
        let package = release(&path, id)?;
        storage::committed(&path, &head(&path)?, expected)?;
        let (_, binding) = read_bundle(&path, expected)?;
        checkpoint::validate_state(&root, &binding)?;
        (root, package, catalog(&path))
    };
    execution::run_revision(&root, &package, &catalog, expected, execute, cancelled)
}

/// Commit state + product + report together, only if the expected head survives.
pub fn commit(
    root: &Path,
    id: &str,
    expected: &str,
    run_id: &str,
    approved: Option<&str>,
) -> Result<Value> {
    commit_selected(root, id, expected, run_id, None, approved)
}

fn commit_selected(
    root: &Path,
    id: &str,
    expected: &str,
    run_id: &str,
    manifest: Option<&checkpoint::Manifest>,
    approved: Option<&str>,
) -> Result<Value> {
    let (root, _lock) = root_lock(root)?;
    let path = instance_path(&root, id)?;
    let package = release(&path, id)?;
    let current = head(&path)?;
    read_bundle(&path, &current)?;
    let accepted = settled_run::accepted(&root, run_id)?;
    let receipt: Value =
        serde_json::from_slice(&regular(&accepted.directory.join("receipt.json"), 65536)?)?;
    let (_, binding) = read_bundle(&path, expected)?;
    if receipt["release"] != package {
        return Err("candidate release/input does not belong to this instance revision".into());
    }
    let prepared = checkpoint::prepare(
        &root,
        id,
        expected,
        &binding,
        &receipt["input_revision"],
        manifest,
    )?;
    Bundle::parse(&accepted.artifact)?;
    if accepted.artifact.len() > 8192 {
        return Err("candidate bundle exceeds next-run input limit".into());
    }
    let mut plan = json!({"schema":"an-instance-commit-v1","store":root,"instance":id,
        "release":package,"expected":expected,"run_id":run_id,"evidence":accepted.evidence});
    if let Some(metadata) = &prepared {
        plan["checkpoint"] = serde_json::to_value(metadata)?;
    }
    let (digest, execute) = approval(&plan, approved)?;
    if current != expected {
        let conflict = json!({"status":"conflict","instance":id,"expected":expected,
            "actual":current,"run_id":run_id,"automatic_retries":0});
        if execute {
            storage::record(&path, &digest, "conflict.json", &conflict)?;
        }
        return Ok(conflict);
    }
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":digest,"executed":false}));
    }
    storage::record(&path, &digest, "intent.json", &plan)?;
    let (bytes, metadata) = checkpoint::stage(&root, run_id, &accepted.artifact, prepared)?;
    let mut source = json!({"kind":"instance-commit","instance":id,"run_id":run_id,
        "approval_sha256":digest,"receipt_sha256":accepted.evidence["receipt_sha256"]});
    if let Some(metadata) = metadata {
        source["checkpoint"] = serde_json::to_value(metadata)?;
    }
    let published =
        artifact_versions::publish(&catalog(&path), &bytes, Some(expected.into()), source)?;
    let revision = published["revision"]
        .as_str()
        .ok_or("missing state revision")?;
    // Publication changes one pointer only after the complete bundle is stored.
    // The store lock serializes cooperating writers; this is not distributed CAS.
    let pending = path.join(format!("HEAD.{digest}.pending"));
    store::write_new(&pending, revision.as_bytes())?;
    fs::rename(&pending, path.join("HEAD"))?;
    let result = json!({"status":"succeeded","instance":id,"parent":expected,
        "revision":revision,"run_id":run_id,"approval_sha256":digest});
    storage::record(&path, &digest, "result.json", &result)?;
    Ok(result)
}
