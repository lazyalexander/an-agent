//! A primary owner claims a VM generation before any guest mutation.
use super::*;

fn claims(session: &Path) -> Result<PathBuf> {
    Ok(session
        .parent()
        .and_then(Path::parent)
        .ok_or("task session root")?
        .join("cocoon-vm-claims"))
}

pub(super) fn claim(
    session: &Path,
    task: &str,
    profile: &Profile,
    attempt: &str,
    spec: &spec::Spec,
) -> Result<()> {
    let directory = claims(session)?;
    store::directory(&directory)?;
    let path = directory.join(format!("{}-{}.json", profile.vm, profile.generation));
    if path.exists() {
        let existing: Value = serde_json::from_slice(&store::read(&path, 32768)?)?;
        if existing["session"] != json!(session)
            || existing["task"] != task
            || existing["attempt"] != attempt
            || existing["transport_sha256"] != spec.transport_sha256
        {
            return Err("Cocoon VM generation belongs to another session or transport".into());
        }
        return Ok(());
    }
    persist(
        &path,
        &json!({"session":session,"task":task,"vm":profile.vm,"generation":profile.generation,
        "attempt":attempt,"transport":spec.transport,"transport_sha256":spec.transport_sha256}),
    )
}

/// Uses the original work grant only for stop and observation after task revocation.
pub(crate) fn release(session: &Path) -> Result<Value> {
    let directory = claims(session)?;
    if !directory.exists() {
        return Ok(Value::Null);
    }
    let completed = session.join("cocoon-release.json");
    if completed.exists() {
        return serde_json::from_slice(&store::read(&completed, 65536)?).map_err(Into::into);
    }
    let mut targets = Vec::new();
    for (index, entry) in fs::read_dir(directory)?.enumerate() {
        if index >= 256 {
            return Err("Cocoon VM claim limit".into());
        }
        let path = entry?.path();
        if path.extension().is_none_or(|v| v != "json") {
            continue;
        }
        let value: Value = serde_json::from_slice(&store::read(&path, 32768)?)?;
        if value["session"] != json!(session) {
            continue;
        }
        let result = (|| -> Result<bool> {
            let profile = super::super::load(
                Path::new(value["transport"].as_str().ok_or("transport")?),
                value["transport_sha256"].as_str().ok_or("transport hash")?,
            )?;
            if value["vm"] != profile.vm || value["generation"] != profile.generation {
                return Err("Cocoon release target mismatch".into());
            }
            let key = uuid::Uuid::parse_str(
                &store::digest(format!("{}:{}:stop", profile.vm, profile.generation).as_bytes())
                    [..32],
            )?
            .to_string();
            let root = session.join("cocoon-release");
            let mut receipt = submit(
                &root,
                &profile,
                &key,
                json!({"operation":"stop","vm":profile.vm}),
            )?;
            if receipt["state"] == "unknown"
                && let Ok(known) = reconcile(&root, &profile, &key)
            {
                receipt = known;
            }
            // A fresh exact-generation observation may confirm quiescence even
            // when the original stop reply was lost. It never replays stop.
            let state = exchange(
                &profile,
                &an_agent_core::det_seam::Entropy::os().uuid_v4().to_string(),
                json!({"operation":"show","vm":profile.vm}),
            )?;
            let confirmed = state["id"] == profile.vm
                && state["generation"] == profile.generation
                && state["active_attempt"] == value["attempt"]
                && state["active_lease"].is_null()
                && matches!(
                    state["state"].as_str(),
                    Some("stopped" | "recovery_required")
                );
            let _ = receipt;
            Ok(confirmed)
        })();
        targets.push(
            json!({"vm":value["vm"],"generation":value["generation"],"attempt":value["attempt"],
            "termination_confirmed":result.unwrap_or(false)}),
        );
    }
    if targets.is_empty() {
        return Ok(Value::Null);
    }
    let confirmed = targets.iter().all(|v| v["termination_confirmed"] == true);
    let result = json!({"termination_confirmed":confirmed,"vms":targets,"automatic_retries":0});
    if confirmed {
        persist(&completed, &result)?;
    }
    Ok(result)
}
