use super::*;

/// Preview binds the offline runtime, source generation and exact disk/image digests.
pub(in crate::application) fn preview(service: &Path, value: &Value) -> Result<Value> {
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
        Some("retire" | "reclaim_vm") => {
            kind = "resource";
            maintenance::action(service, value)?
        }
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
                || crate::application::tasks::view(service, task)?["state"]["termination_confirmed"]
                    != true
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
