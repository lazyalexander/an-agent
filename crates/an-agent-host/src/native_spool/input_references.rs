//! Explicit source claims for native inputs; no deletion or global GC authority.
use super::*;
use crate::artifact_versions::references;
mod coverage;
mod success;

fn owner(root: &Path, name: &str) -> Result<Value> {
    bundle::identifier(name)?;
    let (instance, _) = state(root)?;
    Ok(
        json!({"root":root,"instance":instance["instance"],"release":instance["release"],"run":name}),
    )
}
fn digest(value: &Value) -> Result<String> {
    Ok(store::digest(&serde_json::to_vec(value)?))
}
fn read(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&regular(path, 2 * 1024 * 1024)?)?)
}

/// Called once after exact approval, before the attempt can use disposable inputs.
pub(super) fn acquire(root: &Path, name: &str, start: &Value) -> Result<()> {
    let Some(inputs) = start["workspace"]["artifact_inputs"].as_array() else {
        return Ok(());
    };
    if inputs.is_empty() {
        return Ok(());
    }
    if inputs.len() > 2 {
        return Err("native input reference limit exceeded".into());
    }
    let owner = owner(root, name)?;
    let start_sha256 = digest(start)?;
    let selected: Vec<_> = inputs
        .iter()
        .enumerate()
        .map(|(slot, input)| {
            json!({
        "schema":"an-native-input-reference-v1","owner":owner,"start_sha256":start_sha256,
        "catalog":input["catalog"],"binding":input["binding"],"slot":slot})
        })
        .collect();
    let plan = json!({"schema":"an-native-input-references-v1","owner":owner,
        "start_sha256":start_sha256,"references":selected});
    let dir = root.join("runs").join(name);
    // Intent precedes source claims. Partial acquisition is retained and cannot resume.
    write_json(&dir.join("input-references.json"), &plan)?;
    fs::File::open(&dir)?.sync_all()?;
    for reference in &selected {
        references::acquire(reference)?;
    }
    write_json(
        &dir.join("input-references-registered.json"),
        &json!({"schema":"an-native-input-references-registered-v1","plan_sha256":digest(&plan)?}),
    )?;
    fs::File::open(&dir)?.sync_all()?;
    Ok(())
}

pub(super) fn registration(root: &Path, name: &str) -> Result<Value> {
    bundle::identifier(name)?;
    let dir = root.join("runs").join(name);
    let plan = read(&dir.join("input-references.json"))?;
    if plan["schema"] != "an-native-input-references-v1" || plan["owner"] != owner(root, name)? {
        return Err("input reference owner mismatch".into());
    }
    let selected = plan["references"].as_array().ok_or("reference list")?;
    if selected.is_empty() || selected.len() > 2 {
        return Err("invalid input reference count".into());
    }
    for (slot, reference) in selected.iter().enumerate() {
        if reference["owner"] != plan["owner"]
            || reference["start_sha256"] != plan["start_sha256"]
            || reference["slot"] != json!(slot)
        {
            return Err("input reference identity mismatch".into());
        }
    }
    if read(&dir.join("input-references-registered.json"))?
        != json!({"schema":"an-native-input-references-registered-v1","plan_sha256":digest(&plan)?})
    {
        return Err("input reference registration is unconfirmed".into());
    }
    let start = read(&dir.join("start.json"))?;
    if digest(&start)? != plan["start_sha256"] {
        return Err("input reference start changed".into());
    }
    let inputs = start["workspace"]["artifact_inputs"]
        .as_array()
        .ok_or("selected inputs")?;
    if inputs.len() != selected.len()
        || inputs
            .iter()
            .zip(selected)
            .any(|(a, b)| a["catalog"] != b["catalog"] || a["binding"] != b["binding"])
    {
        return Err("input reference selection changed".into());
    }
    Ok(plan)
}

fn registered(root: &Path, name: &str) -> Result<(Value, Vec<Value>)> {
    let plan = registration(root, name)?;
    let claims = plan["references"]
        .as_array()
        .ok_or("reference list")?
        .iter()
        .map(references::inspect)
        .collect::<Result<Vec<_>>>()?;
    Ok((plan, claims))
}

pub fn inspect(path: &Path, name: &str) -> Result<Value> {
    let root = location(path)?;
    bundle::identifier(name)?;
    let dir = root.join("runs").join(name);
    store::no_links(&dir)?;
    if !dir.is_dir() {
        return Err("unknown run".into());
    }
    if !dir.join("input-references.json").try_exists()? {
        return Ok(
            json!({"status":"succeeded","disposition":"unregistered","collection_authorized":false,
            "scope":"legacy or raw-file input; absence is not a complete reference census"}),
        );
    }
    let (plan, claims) = registered(&root, name)?;
    Ok(
        json!({"status":"succeeded","disposition":"registered","plan":plan,"claims":claims,"collection_authorized":false}),
    )
}

/// Preserve the original cancelled-round release contract and approval identity.
pub fn release_cancelled(path: &Path, name: &str, approved: Option<&str>) -> Result<Value> {
    release(path, name, None, approved)
}

/// Release a successful stateless delivery only after verified consumption.
pub fn release_succeeded(
    path: &Path,
    name: &str,
    workspace: &Path,
    subscription: &str,
    approved: Option<&str>,
) -> Result<Value> {
    release(path, name, Some((workspace, subscription, None)), approved)
}

pub fn release_checkpoint(
    path: &Path,
    name: &str,
    workspace: &Path,
    subscription: &str,
    coverage: &Path,
    approved: Option<&str>,
) -> Result<Value> {
    release(
        path,
        name,
        Some((workspace, subscription, Some(coverage))),
        approved,
    )
}

fn release(
    path: &Path,
    name: &str,
    delivery: Option<(&Path, &str, Option<&Path>)>,
    approved: Option<&str>,
) -> Result<Value> {
    let root = location(path)?;
    let _lock = store::lock(&root)?;
    let (registration, claims) = registered(&root, name)?;
    let result = run::settled(&root, name)?;
    let completion = if let Some((workspace, subscription, coverage)) = delivery {
        Some(success::evidence(
            &root,
            name,
            &result,
            workspace,
            subscription,
            coverage,
        )?)
    } else {
        None
    };
    if (completion.is_none() && result["status"] != "cancelled")
        || result["workspace"]["termination_confirmed"] != true
    {
        return Err(
            "input release requires confirmed cancellation; successful custody stays retained"
                .into(),
        );
    }
    let dir = root.join("runs").join(name);
    let job = dir.join("job");
    let work = read(&job.join("reclaimed.json"))?;
    if read(&dir.join("reclaimed.json"))?
        != json!({"status":"succeeded","instance":result["instance"],
        "release":result["release"],"run":name,"operation":"reclaim","result":work})
    {
        return Err("input release requires bound work collection".into());
    }
    let attempt = result["workspace"]["attempt"].as_str().ok_or("attempt")?;
    let snapshot = crate::workspace_job::reclaim_snapshots_managed(&job, attempt, None)?;
    if snapshot["status"] != "succeeded" || snapshot["inputs_present"] != false {
        return Err("input release requires confirmed snapshot collection".into());
    }
    let mut plan = json!({"schema":"an-native-input-release-v1","registration":registration,
        "settlement_sha256":digest(&result)?,"work_sha256":digest(&work)?,"snapshots":snapshot,
        "payloads_deleted":false,"automatic_retries":0});
    if let Some(completion) = completion {
        plan["schema"] = json!("an-native-success-input-release-v1");
        plan["completion"] = completion;
    }
    let (hash, execute) = approval(&plan, approved)?;
    let intent = dir.join("input-release-intent.json");
    let complete = dir.join("input-released.json");
    if intent.try_exists()? {
        if read(&intent)? != plan || !complete.try_exists()? {
            return Err("input release is uncertain; inspect original evidence, no retry".into());
        }
        let done = read(&complete)?;
        if done != json!({"status":"succeeded","approval_sha256":hash,"payloads_deleted":false})
            || claims
                .iter()
                .any(|c| c["release"]["approval_sha256"] != hash)
        {
            return Err("input release completion evidence changed".into());
        }
        return Ok(
            json!({"status":"succeeded","already_released":true,"executed":false,"result":done}),
        );
    }
    if complete.try_exists()? || claims.iter().any(|c| !c["release"].is_null()) {
        return Err("input release history is inconsistent".into());
    }
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":hash,"executed":false}));
    }
    write_json(&intent, &plan)?;
    fs::File::open(&dir)?.sync_all()?;
    for claim in claims {
        references::release(&claim["reference"], &hash)?;
    }
    let done = json!({"status":"succeeded","approval_sha256":hash,"payloads_deleted":false});
    write_json(&complete, &done)?;
    fs::File::open(&dir)?.sync_all()?;
    Ok(done)
}
