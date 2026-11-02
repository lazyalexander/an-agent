//! Reopen an admitted protocol phase by its exact native start and original protocol record.
use super::*;

pub(in crate::native_spool) fn bound_data(start: &Value) -> Result<Value> {
    let selected = start["workspace"]["artifact_inputs"]
        .as_array()
        .and_then(|v| v.first())
        .ok_or("routed business input")?;
    let mut data = inputs::artifact(
        Path::new(text(selected, "catalog")?),
        text(&selected["binding"], "revision")?,
    )?;
    if data["binding"] != selected["binding"] || data["path"] != selected["path"] {
        return Err("routed artifact selection changed".into());
    }
    let key = match start["routed_supervision"]["kind"].as_str() {
        Some("event") => "event_input",
        Some("peer") => "peer_input",
        _ => return Err("unsupported routed owner".into()),
    };
    data[key] = start
        .get(key)
        .ok_or("missing original route context")?
        .clone();
    Ok(data)
}

pub(in crate::native_spool) fn verify_route(root: &Path, start: &Value) -> Result<()> {
    let Some(route) = start.get("routed_supervision") else {
        return Ok(());
    };
    match route["kind"].as_str() {
        Some("event") => events::verify_supervised(root, start, route),
        Some("peer") => peer::bound_phase(root, start, route).map(drop),
        _ => Err("unsupported routed owner".into()),
    }
}

pub(in crate::native_spool) fn finish_route(
    root: &Path,
    start: &Value,
    result: &Value,
) -> Result<()> {
    let route = &start["routed_supervision"];
    if route["kind"] == "peer" && result.get("workspace").is_some() {
        let (dir, record, phase) = peer::bound_phase(root, start, route)?;
        if run::settled_native(root, text(start, "run")?)? != *result {
            return Err("peer result differs from native settlement".into());
        }
        write_json(
            &dir.join(format!("{phase}-result.json")),
            &json!({"request":record["approval_sha256"],"result":result}),
        )?;
        sync(&dir)?;
    }
    Ok(())
}

/// Protocol/custody callers need both the native group receipt and the original routed owner's completion.
pub(in crate::native_spool) fn verify_settled(
    root: &Path,
    name: &str,
    result: &Value,
) -> Result<()> {
    let run = root.join("runs").join(name);
    let start = read(&run.join("start.json"))?;
    if start.get("routed_supervision").is_none() && result.get("supervisor_request").is_none() {
        return Ok(());
    }
    verify_completion(root, name, &start, result)
}

pub(in crate::native_spool) fn reconcile_binding(
    root: &Path,
    name: &str,
    start: &Value,
    result: &mut Value,
) -> Result<()> {
    if start.get("routed_supervision").is_some() {
        let (_, original) = request(root, name)?;
        result["supervisor_request"] = original["approval_sha256"].clone();
        verify_completion(root, name, start, result)?;
    }
    Ok(())
}

fn verify_completion(root: &Path, name: &str, start: &Value, result: &Value) -> Result<()> {
    let (dir, original) = request(root, name)?;
    let hash = &original["approval_sha256"];
    if original["plan"]["input_kind"] != "routed"
        || original["plan"]["start"] != *start
        || result["supervisor_request"] != *hash
        || read(&dir.join("owner.json"))?["request"] != *hash
        || read(&dir.join("completion.json"))? != json!({"request":hash,"result":result})
    {
        return Err(
            "routed supervision completion is missing or inconsistent; retain the original attempt"
                .into(),
        );
    }
    Ok(())
}
