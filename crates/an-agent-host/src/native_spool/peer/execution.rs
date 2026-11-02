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
    execute_phase(path, name, phase, approved, cancel, None)
}

pub fn execute_supervised(
    path: &Path,
    name: &str,
    phase: &str,
    approved: Option<&str>,
    cancel: &AtomicBool,
    executable: &Path,
) -> Result<Value> {
    execute_phase(path, name, phase, approved, cancel, Some(executable))
}

fn execute_phase(
    path: &Path,
    name: &str,
    phase: &str,
    approved: Option<&str>,
    cancel: &AtomicBool,
    executable: Option<&Path>,
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
    let supervision = executable
        .map(crate::execution::supervisor_process::host_executable)
        .transpose()?
        .map(|exe| {
            json!({"kind":"peer","executable":exe,
        "requester":record["plan"]["source"]["root"],"name":name,"phase":phase})
        });
    let selection = inputs::BoundInput {
        supervision: supervision.as_ref(),
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
    if supervision.is_some() || result.get("plan").is_some() || !marker.try_exists()? {
        return Ok(result);
    }
    write_json(
        &dir.join(format!("{phase}-result.json")),
        &json!({"request":record["approval_sha256"],"result":result}),
    )?;
    sync(&dir)?;
    Ok(result)
}

pub(in crate::native_spool) fn bound_phase<'a>(
    receiver: &Path,
    start: &Value,
    route: &'a Value,
) -> Result<(PathBuf, Value, &'a str)> {
    let phase = text(route, "phase")?;
    let role = match phase {
        "request" => "target",
        "response" => "source",
        _ => return Err("invalid peer supervisor phase".into()),
    };
    let (dir, record) = open(Path::new(text(route, "requester")?), text(route, "name")?)?;
    let marker = read(&dir.join(format!("{phase}-start.json")))?;
    if marker != json!({"request":record["approval_sha256"],"start":start})
        || record["plan"][role]["root"] != json!(receiver)
        || start["peer_input"] != context(&record, phase)
        || start["run"] != run_name(&record, phase)?
    {
        return Err("supervisor start differs from the original peer admission".into());
    }
    Ok((dir, record, phase))
}
