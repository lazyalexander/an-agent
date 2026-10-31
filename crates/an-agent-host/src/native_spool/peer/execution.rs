//! Each phase reuses native execution, fixed-input pins and whole-group settlement.
use super::*;
use std::sync::atomic::AtomicBool;

pub fn execute(
    path: &Path,
    name: &str,
    phase: &str,
    approved: Option<&str>,
    cancel: &AtomicBool,
) -> Result<Value> {
    if !matches!(phase, "request" | "response") {
        return Err("invalid peer phase".into());
    }
    let (dir, record) = open(path, name)?;
    available(&dir, &record)?;
    let marker = dir.join(format!("{phase}-start.json"));
    if phase_started(&dir, &record, phase)? {
        return Err("peer phase admitted or uncertain; inspect, never replay".into());
    }
    let mut data = if phase == "request" {
        let source = Path::new(text(&record["plan"]["source"], "root")?);
        let value = artifact(
            source,
            text(&record["plan"], "source_run")?,
            text(&record["plan"], "source_output")?,
        )?;
        if value != record["plan"]["data"] {
            return Err("peer request material changed".into());
        }
        value
    } else {
        publication::confirmed_reply(&dir, &record)?
    };
    data["peer_input"] = context(&record, phase);
    let role = if phase == "request" {
        "target"
    } else {
        "source"
    };
    let receiver = Path::new(text(&record["plan"][role], "root")?);
    let state_key = format!("{phase}_state");
    let selection = inputs::BoundInput {
        artifact: &data,
        state_revision: record["plan"][&state_key].as_str(),
    };
    let result = run::start_bound(
        receiver,
        &run_name(&record, phase)?,
        selection,
        record["plan"]["wall_seconds"]
            .as_u64()
            .ok_or("peer deadline")?,
        approved,
        cancel,
        |start| {
            // Native owns the receiver lock. The peer lock only covers admission;
            // it never covers execution, so a caller can revoke while work runs.
            let _lock = store::lock(&dir)?;
            available(&dir, &record)?;
            write_json(
                &marker,
                &json!({"request":record["approval_sha256"],"start":start}),
            )?;
            sync(&dir)?;
            // Endpoint changes are monotonic. Verify again after durable intent:
            // a rotation crossing publication fences this stage before spawn.
            // Later rotation cannot transfer an already admitted phase.
            current(&record)
        },
    )?;
    if result.get("plan").is_some() || !marker.try_exists()? {
        return Ok(result);
    }
    write_json(
        &dir.join(format!("{phase}-result.json")),
        &json!({"request":record["approval_sha256"],"result":result}),
    )?;
    sync(&dir)?;
    Ok(result)
}
