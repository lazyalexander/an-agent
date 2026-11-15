//! Operator-only resource views and preview binding; the model never receives cleanup authority.
use super::*;

fn declaration() -> Value {
    json!({"schema":"cocoon-image-maintenance-v1","manual_reclaim":true,"automatic_gc":false})
}
fn supported(runtime: &Path, manifest: &Value) -> Result<bool> {
    let Some(name) = manifest["components"]["image_maintenance_api"].as_str() else {
        return Ok(false);
    };
    if read(&runtime.join(name))? != declaration() {
        return Err("unsupported image maintenance contract".into());
    }
    Ok(true)
}
pub(in crate::application) fn inventory(service: &Path, value: &Value) -> Result<Value> {
    let environment = value["environment"]
        .as_str()
        .ok_or("resource environment required")?;
    let (runtime, manifest) = runtime(service, environment)?;
    if !supported(&runtime, &manifest)? {
        return Ok(json!({"supported":false,"resources":[]}));
    }
    let binary = binary(&runtime, &manifest)?;
    let root = state_root(service, environment)?;
    if query(&binary, &root, "image-maintenance-info", None)? != declaration() {
        return Err("maintenance backend differs from pinned contract".into());
    }
    let mut result = query(&binary, &root, "image-inventory", None)?;
    if result["schema"] != "cocoon-resource-inventory-v1" || result["capabilities"] != declaration()
    {
        return Err("invalid maintenance inventory".into());
    }
    result["supported"] = json!(true);
    result["environment"] = json!(environment);
    Ok(result)
}
pub(super) fn action(service: &Path, value: &Value) -> Result<Value> {
    let inventory = inventory(service, value)?;
    if inventory["supported"] != true {
        return Err("this pinned runtime does not support manual resource cleanup".into());
    }
    let id = uuid(&value["resource"])?;
    let kind = match value["action"].as_str() {
        Some("retire") => "image",
        Some("reclaim_vm") => "vm",
        _ => return Err("unsupported cleanup action".into()),
    };
    let selected = inventory["resources"]
        .as_array()
        .ok_or("resource inventory")?
        .iter()
        .find(|r| r["id"] == id && r["kind"] == kind)
        .ok_or("original resource unavailable")?;
    if selected["reclaimable"] != true {
        return Err("resource is referenced or its original execution remains unresolved".into());
    }
    Ok(if kind == "image" {
        json!({"operation":"retire","image":id,"sha256":selected["sha256"]})
    } else {
        json!({"operation":"reclaim_vm","vm":id,"generation":selected["generation"],"revision":selected["revision"]})
    })
}
