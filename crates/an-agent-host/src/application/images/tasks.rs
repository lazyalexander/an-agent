use super::*;

/// Settle only the original branch request after its tracked control process exits.
pub(in crate::application) fn settle_task_branch(root: &Path) -> Result<bool> {
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
pub(in crate::application) fn ready(service: &Path) -> Result<()> {
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
