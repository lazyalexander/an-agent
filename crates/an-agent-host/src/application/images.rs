//! Host-only cold-disk management. The model receives references, never store credentials.
use crate::execution::{
    Result,
    sandbox::{product_process, service_process},
    store,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn uuid(value: &Value) -> Result<&str> {
    let value = value.as_str().ok_or("image operation identity required")?;
    if uuid::Uuid::parse_str(value)?.to_string() != value {
        return Err("noncanonical identity".into());
    }
    Ok(value)
}
pub(super) fn state_root(service: &Path, environment: &str) -> Result<PathBuf> {
    if !store::valid_digest(environment) {
        return Err("invalid image environment".into());
    }
    Ok(service
        .join("application/images")
        .join(environment)
        .join("cocoon"))
}
fn operation_root(service: &Path, environment: &str, id: &str) -> Result<PathBuf> {
    uuid(&json!(id))?;
    Ok(state_root(service, environment)?
        .parent()
        .ok_or("image namespace")?
        .join("operations")
        .join(id))
}
fn read(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&store::read(path, 131072)?)?)
}
fn runtime_manifest(environment: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&store::read(
        &environment.join("runtime.json"),
        2 * 1024 * 1024,
    )?)?)
}
fn runtime(service: &Path, environment: &str) -> Result<(PathBuf, Value)> {
    state_root(service, environment)?;
    let root = service.join("application/environments").join(environment);
    let verified = super::environment::validate_installed(&root)?;
    if verified["id"] != environment {
        return Err("image runtime changed".into());
    }
    Ok((root, verified["manifest"].clone()))
}
pub(super) fn capable(runtime: &Path, manifest: &Value) -> Result<bool> {
    let Some(component) = manifest["components"]["image_api"].as_str() else {
        return Ok(false);
    };
    let contract = read(&runtime.join(component))?;
    if contract
        != json!({"schema":"cocoon-image-api-v1","disk_images":true,"memory_restore_qualified":false})
    {
        return Err("unsupported image contract".into());
    }
    Ok(true)
}
fn binary(environment: &Path, manifest: &Value) -> Result<PathBuf> {
    Ok(environment.join(
        manifest["components"]["cocoon"]
            .as_str()
            .ok_or("Cocoon component")?,
    ))
}
fn query(binary: &Path, root: &Path, command: &str, argument: Option<&Path>) -> Result<Value> {
    product_process::image_query(binary, root, command, argument)
}
fn image(binary: &Path, root: &Path, reference: &Value) -> Result<Value> {
    let id = uuid(&reference["image"])?;
    let sha = reference["sha256"]
        .as_str()
        .filter(|s| store::valid_digest(s))
        .ok_or("image digest required")?;
    if !root.exists() {
        return Err("image store unavailable".into());
    }
    let selected = query(binary, root, "image-info", Some(Path::new(id)))?;
    if selected["sha256"] != sha {
        return Err("image reference changed".into());
    }
    Ok(selected)
}
pub(super) fn selected(service: &Path, environment: &str, reference: &Value) -> Result<Value> {
    let (runtime, manifest) = runtime(service, environment)?;
    let selected = image(
        &binary(&runtime, &manifest)?,
        &state_root(service, environment)?,
        reference,
    )?;
    if selected["approved"] != true {
        return Err("image requires host approval".into());
    }
    Ok(selected)
}
fn idle(service: &Path) -> Result<()> {
    let active = service.join("application/active-delivery.json");
    if active.exists() {
        let prior = read(&active)?;
        if super::tasks::view(service, uuid(&prior["id"])?)?["state"]["termination_confirmed"]
            != true
        {
            return Err("settle the active delivery before image management".into());
        }
    }
    let host = crate::daemon::request(service, &json!({"op":"show"}))?;
    if host["tasks"]
        .as_array()
        .is_some_and(|t| t.iter().any(|v| v["state"] != "settled"))
    {
        return Err("image management requires an idle service".into());
    }
    if list(service)?.iter().any(|v| {
        !matches!(
            v["state"].as_str(),
            Some("succeeded" | "failed" | "cancelled")
        )
    }) {
        return Err("reconcile the original image operation first".into());
    }
    Ok(())
}
/// Preview binds the offline runtime, source generation and exact disk/image digests.
pub(super) fn preview(service: &Path, value: &Value) -> Result<Value> {
    let id = uuid(&value["id"])?;
    let environment = value["environment"].as_str().ok_or("image environment")?;
    let (runtime, manifest) = runtime(service, environment)?;
    if !capable(&runtime, &manifest)? {
        return Err("this runtime has no qualified disk-image API".into());
    }
    let binary = binary(&runtime, &manifest)?;
    let state = state_root(service, environment)?;
    let mut kind = "project";
    let action = match value["action"].as_str() {
        Some("save_environment") => {
            kind = "environment";
            let mut action = json!({"operation":"import_candidate","environment":environment});
            for (field, component) in [
                ("kernel", "kernel"),
                ("initrd", "initrd"),
                ("disk", "root_image"),
            ] {
                let name = manifest["components"][component]
                    .as_str()
                    .ok_or("image runtime component")?;
                action[field] = json!(runtime.join(name));
                action[format!("{field}_sha256")] = manifest["files"][name]["sha256"].clone();
            }
            action
        }
        Some("save_project") => {
            let task = uuid(&value["task"])?;
            let task_root = service.join("application/tasks").join(task);
            let plan = read(&task_root.join("plan.json"))?;
            if plan["environment_digest"] != environment
                || plan["cocoon_state"] != json!(state)
                || super::tasks::view(service, task)?["state"]["termination_confirmed"] != true
            {
                return Err(
                    "project source must be a terminated task in this private image store".into(),
                );
            }
            let setup = read(&task_root.join("setup.json"))?;
            let vm = uuid(&setup["vm"])?;
            let disk = query(&binary, &state, "disk-info", Some(Path::new(vm)))?;
            json!({"operation":"save_candidate","vm":vm,"generation":disk["generation"],"disk_sha256":disk["disk_sha256"]})
        }
        Some("promote" | "branch" | "rollback") => {
            let selected = image(&binary, &state, &value["image"])?;
            kind = if selected["source_environment"].is_string() {
                "environment"
            } else {
                "project"
            };
            let mut action = json!({"operation":match value["action"].as_str(){Some("promote")=>"promote_candidate",Some("branch")=>"branch",_=>"rollback"},"image":selected["image"],"sha256":selected["sha256"]});
            if value["action"] == "branch" {
                action["vm"] = json!(id);
                action["owner"] = json!(format!("image-branch:{id}"));
            }
            if value["action"] == "rollback" {
                let branch = uuid(&value["branch"])?;
                let original =
                    read(&operation_root(service, environment, branch)?.join("plan.json"))?;
                if original["action"]["operation"] != "branch"
                    || status(service, environment, branch)?["state"] != "succeeded"
                {
                    return Err("rollback requires the original standalone branch".into());
                }
                let vm = uuid(&original["action"]["vm"])?;
                let disk = query(&binary, &state, "disk-info", Some(Path::new(vm)))?;
                action["vm"] = json!(vm);
                action["generation"] = disk["generation"].clone();
                action["disk_sha256"] = disk["disk_sha256"].clone();
            }
            action
        }
        _ => return Err("unsupported image management action".into()),
    };
    let plan = json!({"id":id,"environment":environment,"cocoon_state":state,"binary":binary,"binary_sha256":crate::execution::sandbox::service_process::executable_hash(&binary)?,"kind":kind,"action":action,"memory_restore_qualified":false});
    Ok(
        json!({"plan":plan,"approval_sha256":store::digest(&serde_json::to_vec(&plan)?),"executed":false}),
    )
}
pub(super) fn submit(service: &Path, value: &Value) -> Result<Value> {
    let id = uuid(&value["id"])?;
    let environment = value["environment"].as_str().ok_or("image environment")?;
    let root = operation_root(service, environment, id)?;
    let application = service.join("application");
    let _lock = store::lock(&application)?;
    if root.exists() {
        let plan = read(&root.join("plan.json"))?;
        if value["approve"] != store::digest(&serde_json::to_vec(&plan)?) {
            return Err("image operation identity already bound".into());
        }
        return status(service, environment, id);
    }
    idle(service)?;
    let checked = preview(service, value)?;
    if value["approve"] != checked["approval_sha256"] {
        return Err("image approval changed".into());
    }
    let plan = &checked["plan"];
    let namespace = state_root(service, environment)?
        .parent()
        .ok_or("namespace")?
        .to_owned();
    store::directory(&application.join("images"))?;
    store::directory(&namespace)?;
    store::directory(&namespace.join("operations"))?;
    if fs::read_dir(namespace.join("operations"))?
        .take(257)
        .count()
        >= 256
    {
        return Err("image operation registry full".into());
    }
    store::directory(&root)?;
    store::write_new(&root.join("plan.json"), &serde_json::to_vec(plan)?)?;
    let binary = Path::new(plan["binary"].as_str().ok_or("image binary")?);
    let state = state_root(service, environment)?;
    let capabilities = query(binary, &state, "image-init", None)?;
    if capabilities["schema"] != "cocoon-image-api-v1"
        || capabilities["memory_restore_qualified"] != false
    {
        return Err("unqualified image management backend".into());
    }
    let credential = String::from_utf8(store::read(&state.join("api-credential"), 4096)?)?;
    let request = json!({"request":id,"caller":"an-agent-image-host","credential":credential,"action":plan["action"]});
    store::write_new(&root.join("request.json"), &serde_json::to_vec(&request)?)?;
    let mut query = request;
    query.as_object_mut().ok_or("query")?.remove("action");
    store::write_new(&root.join("query.json"), &serde_json::to_vec(&query)?)?;
    store::write_new(&root.join("launch.json"), b"{\"automatic_retries\":0}")?;
    let log = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join("control.log"))?;
    let child = product_process::image_submit(binary, &state, &root.join("request.json"), log)?;
    store::write_new(
        &root.join("owner.json"),
        &serde_json::to_vec(&service_process::identity(child.id()))?,
    )?;
    fs::File::open(&root)?.sync_all()?;
    product_process::reap(child)?;
    Ok(json!({"id":id,"environment":environment,"state":"running","kind":plan["kind"]}))
}
pub(super) fn status(service: &Path, environment: &str, id: &str) -> Result<Value> {
    let root = operation_root(service, environment, id)?;
    let cache = root.join("settled.json");
    if cache.exists() {
        return read(&cache);
    }
    let plan = read(&root.join("plan.json"))?;
    let binary = Path::new(plan["binary"].as_str().ok_or("image binary")?);
    let state = state_root(service, environment)?;
    // A missing launch fence or backend receipt never authorizes another launch.
    let record = if root.join("query.json").exists() {
        query(
            binary,
            &state,
            "image-status",
            Some(&root.join("query.json")),
        )
        .unwrap_or_else(|_| json!({"state":"unknown"}))
    } else {
        json!({"state":"unknown"})
    };
    let value = json!({"id":id,"environment":environment,"kind":plan["kind"],"action":plan["action"]["operation"],"state":record["state"],"receipt":record});
    if matches!(
        value["state"].as_str(),
        Some("succeeded" | "failed" | "cancelled")
    ) {
        let temporary = root.join(format!(
            ".settled-{}",
            an_agent_core::det_seam::Entropy::os().uuid_v4()
        ));
        store::write_new(&temporary, &serde_json::to_vec(&value)?)?;
        let publication = fs::hard_link(&temporary, &cache);
        fs::remove_file(temporary)?;
        if let Err(error) = publication
            && error.kind() != std::io::ErrorKind::AlreadyExists
        {
            return Err(error.into());
        }
        fs::File::open(&root)?.sync_all()?;
    }
    Ok(value)
}
pub(super) fn control(service: &Path, value: &Value) -> Result<Value> {
    let environment = value["environment"].as_str().ok_or("image environment")?;
    let id = uuid(&value["id"])?;
    let root = operation_root(service, environment, id)?;
    let plan = read(&root.join("plan.json"))?;
    let command = match value["control"].as_str() {
        Some("cancel") => "image-cancel",
        Some("reconcile") => "image-reconcile",
        _ => return Err("image control must query the original operation".into()),
    };
    query(
        Path::new(plan["binary"].as_str().ok_or("image binary")?),
        &state_root(service, environment)?,
        command,
        Some(&root.join("query.json")),
    )?;
    status(service, environment, id)
}
pub(super) fn list(service: &Path) -> Result<Vec<Value>> {
    let directory = service.join("application/images");
    let mut result = Vec::new();
    if !directory.exists() {
        return Ok(result);
    }
    for namespace in fs::read_dir(directory)?.take(257) {
        let namespace = namespace?;
        let environment = namespace.file_name().to_string_lossy().into_owned();
        if !store::valid_digest(&environment) {
            continue;
        }
        let operations = namespace.path().join("operations");
        if !operations.exists() {
            continue;
        }
        for entry in fs::read_dir(operations)?.take(257) {
            if result.len() >= 256 {
                return Err("image registry limit".into());
            }
            let id = entry?.file_name().to_string_lossy().into_owned();
            if uuid::Uuid::parse_str(&id).is_ok() {
                result.push(status(service, &environment, &id)?);
            }
        }
    }
    Ok(result)
}
/// Settle only the original branch request after its tracked control process exits.
pub(super) fn settle_task_branch(root: &Path) -> Result<bool> {
    if !root.join("image-branch-intent.json").exists() {
        return Ok(true);
    }
    let owner = read(&root.join("runtime-owner.json"))?;
    let pid = owner["identity"]["pid"]
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .ok_or("original image control identity")?;
    if !service_process::exited(pid) {
        return Ok(false);
    }
    let plan = read(&root.join("plan.json"))?;
    let environment = Path::new(
        plan["environment"]
            .as_str()
            .ok_or("original image environment")?,
    );
    let manifest = runtime_manifest(environment)?;
    let binary = binary(environment, &manifest)?;
    let component = manifest["components"]["cocoon"]
        .as_str()
        .ok_or("original Cocoon component")?;
    if service_process::executable_hash(&binary)? != manifest["files"][component]["sha256"] {
        return Err("original image backend changed".into());
    }
    let state = Path::new(
        plan["cocoon_state"]
            .as_str()
            .ok_or("original image store")?,
    );
    let path = root.join("image-branch-query.json");
    let original = read(&root.join("image-branch-intent.json"))?;
    if read(&path)?["request"] != original["request"] {
        return Err("original image request binding changed".into());
    }
    let mut result = query(&binary, state, "image-cancel", Some(&path))?;
    if result["state"] == "unknown" {
        result = query(&binary, state, "image-reconcile", Some(&path))?;
    }
    Ok(matches!(
        result["state"].as_str(),
        Some("succeeded" | "failed" | "cancelled")
    ))
}
/// Task admission excludes in-flight image mutation; legacy task stores stay untouched.
pub(super) fn ready(service: &Path) -> Result<()> {
    if list(service)?.iter().any(|v| {
        !matches!(
            v["state"].as_str(),
            Some("succeeded" | "failed" | "cancelled")
        )
    }) {
        return Err("reconcile the image operation before starting a task".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
