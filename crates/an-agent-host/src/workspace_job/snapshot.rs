//! Attempt-private selected input bytes; runtime dependencies remain operator owned.
use crate::{
    execution::{Result, sandbox, store},
    file_job::regular,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

pub(super) fn launch(plan: &Value, attempt: &Path) -> Result<Value> {
    if plan["schema"] != "an-workspace-plan-v2" {
        return Err("workspace plan lacks immutable input snapshot authority".into());
    }
    let snapshot = attempt.join("snapshot");
    let input_root = snapshot.join("inputs");
    let mut launch = plan.clone();
    let mut inputs = Vec::new();
    for input in plan["inputs"].as_array().ok_or("missing selected inputs")? {
        let name = input["name"].as_str().ok_or("missing snapshot name")?;
        super::inventory::relative(name)?;
        if Path::new(name).components().count() != 1 {
            return Err("snapshot name must be a basename".into());
        }
        inputs.push(json!({"path":input_root.join(name),"sha256":input["sha256"],"name":name}));
    }
    let resolve = |text: &str| -> Result<String> {
        let mut result = text.replace("{inputs}", &input_root.to_string_lossy());
        for (index, input) in inputs.iter().enumerate() {
            result = result.replace(
                &format!("{{input:{index}}}"),
                input["path"].as_str().ok_or("input path")?,
            );
        }
        if result.contains("{input:") {
            return Err("unknown input placeholder".into());
        }
        Ok(result)
    };
    launch["args"] = Value::Array(
        plan["args"]
            .as_array()
            .ok_or("missing args")?
            .iter()
            .map(|v| resolve(v.as_str().ok_or("invalid arg")?).map(Value::String))
            .collect::<Result<_>>()?,
    );
    let mut environment = serde_json::Map::new();
    for (key, value) in plan["environment"]
        .as_object()
        .ok_or("missing environment")?
    {
        environment.insert(
            key.clone(),
            json!(resolve(value.as_str().ok_or("environment value")?)?),
        );
    }
    launch["environment"] = Value::Object(environment);
    launch["inputs"] = json!(inputs);
    let mut roots: Vec<PathBuf> = plan["readonly"]
        .as_array()
        .ok_or("missing runtime roots")?
        .iter()
        .map(|v| {
            v.as_str()
                .map(PathBuf::from)
                .ok_or_else(|| "invalid runtime root".into())
        })
        .collect::<Result<_>>()?;
    roots.push(input_root);
    launch["policy"] = json!(sandbox::workspace::policy(
        Path::new(launch["executable"].as_str().ok_or("executable")?),
        &Path::new(plan["root"].as_str().ok_or("root")?).join("work"),
        &roots,
        &plan["inputs"]
            .as_array()
            .ok_or("inputs")?
            .iter()
            .map(|i| i["path"]
                .as_str()
                .map(PathBuf::from)
                .ok_or_else(|| "source path".into()))
            .collect::<Result<Vec<_>>>()?
    )?);
    Ok(launch)
}

pub(super) fn prepare(plan: &Value, attempt: &Path, cancelled: &AtomicBool) -> Result<Value> {
    if cancelled.load(std::sync::atomic::Ordering::Acquire) {
        return Err("snapshot preparation cancelled; no workload started".into());
    }
    let execution = launch(plan, attempt)?;
    let snapshot = attempt.join("snapshot");
    store::directory(&snapshot)?;
    store::directory(&snapshot.join("inputs"))?;
    for (source, target) in plan["inputs"]
        .as_array()
        .ok_or("inputs")?
        .iter()
        .zip(execution["inputs"].as_array().ok_or("snapshot inputs")?)
    {
        if cancelled.load(std::sync::atomic::Ordering::Acquire) {
            return Err("snapshot preparation cancelled; no workload started".into());
        }
        let pinned = plan["artifact_inputs"]
            .as_array()
            .and_then(|items| items.iter().find(|i| i["path"] == source["path"]));
        let path = Path::new(target["path"].as_str().ok_or("snapshot path")?);
        let cloned = pinned.is_none()
            && super::snapshot_clone::create(
                Path::new(source["path"].as_str().ok_or("source")?),
                path,
                64 * 1024 * 1024,
            )?;
        if cancelled.load(std::sync::atomic::Ordering::Acquire) {
            return Err("snapshot preparation cancelled; no workload started".into());
        }
        let bytes = if let Some(item) = pinned {
            let (bytes, binding) = crate::artifact_versions::read(
                Path::new(item["catalog"].as_str().ok_or("catalog")?),
                item["binding"]["revision"].as_str().ok_or("revision")?,
            )?;
            if binding != item["binding"] {
                return Err("pinned artifact binding changed".into());
            }
            bytes
        } else if cloned {
            regular(path, 64 * 1024 * 1024)?
        } else {
            regular(
                Path::new(source["path"].as_str().ok_or("source")?),
                64 * 1024 * 1024,
            )?
        };
        if store::digest(&bytes) != source["sha256"] {
            return Err("approved input changed before snapshot".into());
        }
        if !cloned {
            store::write_new(path, &bytes)?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o400))?;
        }
        fs::File::open(path)?.sync_all()?;
    }
    if cancelled.load(std::sync::atomic::Ordering::Acquire) {
        return Err("snapshot preparation cancelled; no workload started".into());
    }
    super::profile::verify_inputs(&execution)?;
    #[cfg(unix)]
    {
        fs::File::open(snapshot.join("inputs"))?.sync_all()?;
        fs::File::open(&snapshot)?.sync_all()?;
    }
    let bytes = serde_json::to_vec_pretty(&execution)?;
    store::write_new(&attempt.join("execution.json"), &bytes)?;
    #[cfg(unix)]
    fs::File::open(attempt)?.sync_all()?;
    Ok(execution)
}

pub(super) fn verified(attempt: &Path, plan: &Value, receipt: &Value) -> Result<Value> {
    let bytes = regular(&attempt.join("execution.json"), 2 * 1024 * 1024)?;
    if receipt["execution_sha256"] != store::digest(&bytes) {
        return Err("missing or changed snapshot execution binding".into());
    }
    let execution: Value = serde_json::from_slice(&bytes)?;
    if execution != launch(plan, attempt)? {
        return Err("execution snapshot differs from approved mapping".into());
    }
    super::profile::verify_inputs(&execution)?;
    Ok(execution)
}

#[cfg(test)]
#[path = "snapshot_tests.rs"]
mod tests;
