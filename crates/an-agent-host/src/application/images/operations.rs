use super::*;

fn idle(service: &Path) -> Result<()> {
    let active = service.join("application/active-delivery.json");
    if active.exists() {
        let prior = read(&active)?;
        if crate::application::tasks::view(service, uuid(&prior["id"])?)?["state"]["termination_confirmed"]
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
pub(in crate::application) fn submit(service: &Path, value: &Value) -> Result<Value> {
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
pub(in crate::application) fn status(service: &Path, environment: &str, id: &str) -> Result<Value> {
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
pub(in crate::application) fn control(service: &Path, value: &Value) -> Result<Value> {
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
pub(in crate::application) fn list(service: &Path) -> Result<Vec<Value>> {
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
