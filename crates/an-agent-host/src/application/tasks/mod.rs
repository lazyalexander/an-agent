//! Durable product intents reference existing host jobs; they are not execution receipts.
pub mod runtime;
use crate::execution::{Result, store};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn directory(service: &Path, id: &str) -> Result<PathBuf> {
    if uuid::Uuid::parse_str(id)?.to_string() != id {
        return Err("invalid product task identity".into());
    }
    let root = service.join("application/tasks");
    store::directory(&root)?;
    Ok(root.join(id))
}
pub(super) fn preview(service: &Path, req: &Value) -> Result<Value> {
    let id = req["id"].as_str().ok_or("task identity required")?;
    directory(service, id)?;
    let environment = req["environment"].as_str().ok_or("environment required")?;
    if !store::valid_digest(environment) {
        return Err("invalid environment identity".into());
    }
    let environment = service.join("application/environments").join(environment);
    let verified = super::environment::validate_installed(&environment)?;
    if verified["id"] != req["environment"] {
        return Err("environment identity changed".into());
    }
    let components = &verified["manifest"]["components"];
    for name in [
        "delivery",
        "cocoon",
        "runner",
        "kernel",
        "initrd",
        "root_image",
        "native_python",
        "native_stdlib",
        "native_site",
        "intermediate_package",
        "final_package",
    ] {
        if components[name].as_str().is_none() {
            return Err(format!("missing environment component: {name}").into());
        }
    }
    let upload = req["upload"].as_str().ok_or("input upload required")?;
    if uuid::Uuid::parse_str(upload)?.to_string() != upload {
        return Err("invalid input identity".into());
    }
    let input = service.join("application/uploads").join(upload);
    let bytes = store::read(&input, 512 * 1024)?;
    if !bytes.starts_with(b"%PDF-") {
        return Err("PDF input required".into());
    }
    let instruction = req["instruction"].as_str().ok_or("instruction required")?;
    if instruction.is_empty() || instruction.len() > 65536 {
        return Err("bounded task instruction required".into());
    }
    let home = std::env::var_os("HOME").ok_or("user home unavailable")?;
    let codex = PathBuf::from(home)
        .join(".codex/packages/standalone/releases/0.166.0-aarch64-apple-darwin/bin/codex");
    let codex = codex
        .canonicalize()
        .map_err(|_| "qualified Codex 0.166.0 is not installed")?;
    let executable = crate::execution::sandbox::service_process::executable_hash(&codex)?;
    let config: Value = serde_json::from_slice(&store::read(&service.join("config.json"), 65536)?)?;
    if config["concurrency"].as_u64().unwrap_or(0) < 2 {
        return Err("document tasks require two queue slots".into());
    }
    let plan = json!({"id":id,"environment":environment,"environment_digest":verified["id"],"input":input,
        "input_sha256":store::digest(&bytes),"instruction":instruction,"codex":codex,"codex_sha256":executable,
        "model":"gpt-6.1-sol","wall_seconds":600,"maximum_turns":3});
    Ok(
        json!({"approval_sha256":store::digest(&serde_json::to_vec(&plan)?),"plan":plan,"executed":false}),
    )
}
pub(super) fn create(service: &Path, req: &Value) -> Result<Value> {
    let id = req["id"].as_str().ok_or("task identity")?;
    let prior = directory(service, id)?.join("plan.json");
    if prior.exists() {
        let original: Value = serde_json::from_slice(&store::read(&prior, 131072)?)?;
        let expected = store::digest(&serde_json::to_vec(&original)?);
        let upload = Path::new(original["input"].as_str().ok_or("original input")?)
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or("upload")?;
        if req["approve"] != expected
            || req["environment"] != original["environment_digest"]
            || req["upload"] != upload
            || req["instruction"] != original["instruction"]
        {
            return Err("task identity already bound to different content".into());
        }
        return Ok(json!({"id":id,"replayed":false}));
    }
    let active = service.join("application/active-delivery.json");
    if active.exists() {
        let selected: Value = serde_json::from_slice(&store::read(&active, 65536)?)?;
        if selected["id"] != id
            && view(service, selected["id"].as_str().ok_or("active task")?)?["state"]["termination_confirmed"]
                != true
        {
            return Err("finish or reconcile the active document task first".into());
        }
    }
    let preview = preview(service, req)?;
    if req["approve"] != preview["approval_sha256"] {
        return Err("task approval changed".into());
    }
    let id = req["id"].as_str().ok_or("task identity")?;
    let root = directory(service, id)?;
    let application = service.join("application");
    let _admission = store::lock(&application)?;
    if fs::read_dir(application.join("tasks"))?.take(256).count() >= 256 {
        return Err("product task registry full".into());
    }
    let active = application.join("active-delivery.json");
    if active.exists() {
        let selected: Value = serde_json::from_slice(&store::read(&active, 65536)?)?;
        if selected["id"] != id {
            let prior = view(service, selected["id"].as_str().ok_or("active task")?)?;
            if prior["state"]["termination_confirmed"] != true {
                return Err("finish or reconcile the active document task first".into());
            }
            fs::remove_file(&active)?;
        }
    }
    if !active.exists() {
        let service_view = crate::daemon::request(service, &json!({"op":"show"}))?;
        if service_view["tasks"]
            .as_array()
            .is_some_and(|tasks| tasks.iter().any(|task| task["state"] != "settled"))
        {
            return Err("document delivery requires an idle service".into());
        }
        store::write_new(&active, &serde_json::to_vec(&json!({"id":id}))?)?;
    }
    if root.exists() {
        let original: Value =
            serde_json::from_slice(&store::read(&root.join("plan.json"), 131072)?)?;
        if original != preview["plan"] {
            return Err("task identity already bound".into());
        }
        return Ok(json!({"id":id,"replayed":false}));
    }
    store::directory(&root)?;
    let plan = &preview["plan"];
    store::write_new(&root.join("plan.json"), &serde_json::to_vec(plan)?)?;
    let bytes = store::read(
        Path::new(plan["input"].as_str().ok_or("input")?),
        4 * 1024 * 1024,
    )?;
    if store::digest(&bytes) != plan["input_sha256"] {
        return Err("input changed".into());
    }
    store::write_new(&root.join("input"), &bytes)?;
    store::write_new(&root.join("launch.json"), b"{\"automatic_retries\":0}")?;
    let log = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join("controller.log"))?;
    let child = crate::execution::sandbox::product_process::task(service, id, log)?;
    let identity = crate::execution::sandbox::service_process::identity(child.id());
    store::write_new(
        &root.join("owner.json"),
        &serde_json::to_vec(&json!({"pid":child.id(),"identity":identity}))?,
    )?;
    Ok(json!({"id":id,"replayed":false}))
}
pub fn worker(service: &Path, id: &str) -> Result<Value> {
    let root = directory(service, id)?;
    let plan: Value = serde_json::from_slice(&store::read(&root.join("plan.json"), 131072)?)?;
    let environment = Path::new(plan["environment"].as_str().ok_or("environment")?);
    let verified = super::environment::validate_installed(environment)?;
    if verified["id"] != plan["environment_digest"] {
        return Err("approved environment changed".into());
    }
    let python = environment.join(
        verified["manifest"]["components"]["native_python"]
            .as_str()
            .ok_or("native Python")?,
    );
    let script = root.join("controller.py");
    store::write_new(&script, include_bytes!("controller.py"))?;
    crate::execution::sandbox::product_process::replace_controller(&python, &script, service, &root)
}

pub(super) fn view(service: &Path, id: &str) -> Result<Value> {
    let root = directory(service, id)?;
    let path = root.join("state.json");
    let mut state = if path.exists() {
        serde_json::from_slice(&store::read(&path, 131072)?)?
    } else {
        json!({"state":"preparing"})
    };
    let owner = root.join("owner.json");
    if owner.exists()
        && !matches!(
            state["state"].as_str(),
            Some("succeeded" | "failed" | "cancelled" | "unknown")
        )
    {
        let owner: Value = serde_json::from_slice(&store::read(&owner, 65536)?)?;
        let pid = owner["pid"]
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .ok_or("controller identity")?;
        let actual = crate::execution::sandbox::service_process::identity(pid);
        if actual.is_null() || actual != owner["identity"] {
            state["state"] = json!("unknown");
            state["error"] = json!("original coordinator unavailable; no replay");
        }
    }
    Ok(json!({"id":id,"state":state}))
}
pub(super) fn clarify(service: &Path, req: &Value) -> Result<Value> {
    let id = req["id"].as_str().ok_or("task identity")?;
    let root = directory(service, id)?;
    let _clarification = store::lock(&root)?;
    let retained = root.join("clarification.json");
    if retained.exists() {
        let plan: Value = serde_json::from_slice(&store::read(&retained, 65536)?)?;
        if plan["answers"] != req["answers"] {
            return Err("clarification already bound to different answers".into());
        }
        let hash = store::digest(&serde_json::to_vec(&plan)?);
        if req.get("approve").is_some() && req["approve"] != hash {
            return Err("clarification approval changed".into());
        }
        return Ok(json!({"plan":plan,"approval_sha256":hash,"submitted":true,"replayed":false}));
    }
    let state = view(service, id)?;
    if state["state"]["state"] != "waiting-clarification" {
        return Err("task is not awaiting clarification".into());
    }
    let answers = req["answers"]
        .as_object()
        .ok_or("clarification answers required")?;
    let keys = state["state"]["questions"]
        .as_array()
        .ok_or("clarification questions")?;
    if answers.len() != keys.len()
        || keys
            .iter()
            .any(|key| key.as_str().is_none_or(|k| !answers.contains_key(k)))
    {
        return Err("clarification keys do not match".into());
    }
    for (key, value) in answers {
        if (key == "currency" && value != "EUR")
            || (key.starts_with("quantity:")
                && !value.as_u64().is_some_and(|n| (1..=1000000).contains(&n)))
            || (key != "currency" && !key.starts_with("quantity:"))
        {
            return Err("unsupported clarification value".into());
        }
    }
    let plan = json!({"id":id,"answers":answers,"questions":keys});
    let hash = store::digest(&serde_json::to_vec(&plan)?);
    if req.get("approve").is_none() {
        return Ok(json!({"plan":plan,"approval_sha256":hash,"executed":false}));
    }
    if req["approve"] != hash {
        return Err("clarification approval changed".into());
    }
    let pending = root.join(".clarification.pending");
    store::write_new(&pending, &serde_json::to_vec(&plan)?)?;
    fs::rename(pending, &retained)?;
    fs::File::open(&root)?.sync_all()?;
    Ok(json!({"submitted":true}))
}
mod reconcile;
pub(super) use reconcile::reconcile;

pub(super) fn cancel(service: &Path, id: &str) -> Result<Value> {
    let root = directory(service, id)?;
    let current = view(service, id)?;
    if current["state"]["termination_confirmed"] == true {
        return Ok(json!({"cancellation_requested":false,"settled":true}));
    }
    if !root.join("cancel.json").exists() {
        store::write_new(&root.join("cancel.json"), b"{}")?;
    }
    let state = view(service, id)?;
    if let Some(session) = state["state"]["session"].as_str() {
        let req = json!({"op":"cancel","id":session});
        let preview = crate::daemon::request(service, &req)?;
        crate::daemon::request(
            service,
            &json!({"op":"cancel","id":session,"approve":preview["approval_sha256"]}),
        )?;
    }
    Ok(json!({"cancellation_requested":true}))
}
pub(super) fn artifact(service: &Path, id: &str, stage: &str) -> Result<Value> {
    let value = view(service, id)?;
    if value["state"]["state"] != "succeeded" || !matches!(stage, "intermediate" | "final") {
        return Err("completed accepted product required".into());
    }
    let reference = &value["state"]["products"][stage];
    let instance = Path::new(reference["instance"].as_str().ok_or("artifact instance")?);
    let run = reference["run"].as_str().ok_or("artifact run")?;
    let output = reference["output"].as_str().ok_or("artifact output")?;
    let inventory = crate::native_spool::artifact_output(instance, run, output)?;
    let revision = inventory["output"]["binding"]["revision"]
        .as_str()
        .ok_or("accepted revision")?;
    let catalog = Path::new(inventory["catalog"].as_str().ok_or("artifact catalog")?);
    let (bytes, _) = crate::artifact_versions::admission::read_input(catalog, revision)?;
    Ok(json!({"bytes":bytes,"content_sha256":store::digest(&bytes),"revision":revision}))
}

#[cfg(test)]
mod tests;
