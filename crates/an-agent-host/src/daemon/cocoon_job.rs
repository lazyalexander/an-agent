//! Authorized fixed programs receive task data; domain rules remain in spool code.
use super::*;

pub(super) fn run(
    root: &Path,
    session: &Path,
    authority: &Binding,
    req: &Value,
    stopped: &AtomicBool,
) -> Result<Value> {
    let spec_sha256 = text(req, "spec_sha256")?;
    crate::task_permissions::require(
        authority,
        &crate::task_permissions::Permission::CocoonJob {
            spec_sha256: spec_sha256.into(),
        },
    )?;
    let inputs = req["inputs"]
        .as_array()
        .ok_or("Cocoon job inputs required")?;
    if inputs.len() > 4 {
        return Err("Cocoon job input count limit".into());
    }
    let mut selected = Vec::new();
    for input in inputs {
        let allowed: &[&str] = match input["op"].as_str() {
            Some("read-input") => &["op", "path", "sha256"],
            Some("artifact-read") => &["op", "instance", "run", "output"],
            _ => return Err("Cocoon job input requires an authorized byte reference".into()),
        };
        if input.as_object().is_none_or(|object| {
            object.len() != allowed.len()
                || object.keys().any(|key| !allowed.contains(&key.as_str()))
        }) {
            return Err("unexpected Cocoon job input fields".into());
        }
        let response = permissions::dispatch_bound(root, input, authority)?;
        selected.push(crate::cocoon::job::Input {
            bytes: serde_json::from_value(response["bytes"].clone())?,
            evidence: input.clone(),
        });
    }
    let task = crate::task_permissions::load(authority)?.scope.task;
    crate::cocoon::job::run(
        session,
        Path::new(text(req, "spec")?),
        spec_sha256,
        text(req, "operation_id")?,
        &task,
        req["data"].clone(),
        selected,
        || {
            if stopped.load(std::sync::atomic::Ordering::Acquire)
                || session.join("cancel.json").exists()
            {
                return Err("primary task cancelled".into());
            }
            crate::task_permissions::active(authority)
        },
    )
}
