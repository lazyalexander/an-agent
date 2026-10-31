//! Host-approved, split-phase peer exchange. No shared workspace mailbox.
//! These operator APIs are not credentials for untrusted spool processes.
use super::*;
mod execution;
mod publication;
pub use execution::execute;
pub use publication::{cancel, reply, request};

fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key]
        .as_str()
        .ok_or_else(|| format!("missing peer {key}").into())
}
fn hash(v: &Value) -> Result<String> {
    Ok(store::digest(&serde_json::to_vec(v)?))
}
fn read(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&regular(path, 2 * 1024 * 1024)?)?)
}
fn sync(path: &Path) -> Result<()> {
    fs::File::open(path)?.sync_all()?;
    Ok(())
}
fn endpoint(root: &Path) -> Result<Value> {
    let (instance, _) = active(root)?;
    Ok(
        json!({"root":root,"instance":instance["instance"],"release":instance["release"],
        "generation":round_control::current(root,&instance)?["generation"]}),
    )
}
fn current(record: &Value) -> Result<()> {
    for role in ["source", "target"] {
        let expected = &record["plan"][role];
        let root = location(Path::new(text(expected, "root")?))?;
        if endpoint(&root)? != *expected {
            return Err("peer endpoint changed; old work cannot claim a new activation".into());
        }
    }
    Ok(())
}
fn available(dir: &Path, record: &Value) -> Result<()> {
    if dir.join("cancelled.json").try_exists()? {
        return Err("peer request revoked; execution and reply admission are closed".into());
    }
    current(record)
}
fn open(path: &Path, name: &str) -> Result<(PathBuf, Value)> {
    bundle::identifier(name)?;
    let root = location(path)?;
    let dir = root.join("peers").join(name);
    store::no_links(&dir)?;
    let record = read(&dir.join("request.json"))?;
    let (instance, _) = state(&root)?;
    if record["plan"]["schema"] != "an-native-peer-request-v1"
        || record["plan"]["name"] != name
        || record["plan"]["source"]["root"] != json!(root)
        || record["plan"]["source"]["instance"] != instance["instance"]
        || record["plan"]["source"]["release"] != instance["release"]
        || record["approval_sha256"] != hash(&record["plan"])?
    {
        return Err("peer request identity mismatch".into());
    }
    Ok((dir, record))
}
fn artifact(root: &Path, run: &str, output: &str) -> Result<Value> {
    bundle::identifier(run)?;
    let report = artifacts::verified(root, run, true)?.0;
    let selected = report["outputs"]
        .as_array()
        .ok_or("accepted outputs")?
        .iter()
        .find(|v| v["path"] == output)
        .ok_or("peer output must be formally accepted")?;
    let value = inputs::artifact(
        Path::new(text(&report, "catalog")?),
        text(&selected["binding"], "revision")?,
    )?;
    // Acceptance carries status; the canonical read includes source provenance.
    // verified() already checks that provenance against the original execution.
    if ["revision", "content_sha256", "bytes", "encoding"]
        .iter()
        .any(|key| value["binding"][key] != selected["binding"][key])
    {
        return Err("peer artifact identity mismatch".into());
    }
    Ok(value)
}
fn run_name(record: &Value, phase: &str) -> Result<String> {
    hash(&json!({"peer":record["approval_sha256"],"phase":phase}))
}
// A lost peer record must not make a real native attempt look unstarted.
// Approval previews can create a run directory without creating any of these files.
fn phase_started(dir: &Path, record: &Value, phase: &str) -> Result<bool> {
    let role = if phase == "request" {
        "target"
    } else {
        "source"
    };
    let run = Path::new(text(&record["plan"][role], "root")?)
        .join("runs")
        .join(run_name(record, phase)?);
    for evidence in [
        dir.join(format!("{phase}-start.json")),
        dir.join(format!("{phase}-result.json")),
        run.join("start.json"),
        run.join("result.json"),
        run.join("job"),
    ] {
        if evidence.try_exists()? {
            return Ok(true);
        }
    }
    Ok(false)
}
fn context(record: &Value, phase: &str) -> Value {
    json!({"schema":"an-native-peer-input-v1","request":record["approval_sha256"],
        "source":record["plan"]["source"],"target":record["plan"]["target"],"phase":phase})
}
fn result(dir: &Path, record: &Value, phase: &str) -> Result<Value> {
    let role = if phase == "request" {
        "target"
    } else {
        "source"
    };
    let root = Path::new(text(&record["plan"][role], "root")?);
    let run = run_name(record, phase)?;
    let intent = read(&dir.join(format!("{phase}-start.json")))?;
    let receipt = read(&dir.join(format!("{phase}-result.json")))?;
    let settled = run::settled(root, &run)?;
    let start = read(&root.join("runs").join(&run).join("start.json"))?;
    if receipt["result"] != settled
        || receipt["request"] != record["approval_sha256"]
        || intent["request"] != record["approval_sha256"]
        || intent["start"] != start
        || start["peer_input"] != context(record, phase)
        || settled["workspace"]["termination_confirmed"] != true
    {
        return Err("peer completion lacks bound termination evidence".into());
    }
    Ok(settled)
}

/// No lock or model call is needed to observe a wait. Observation grants nothing.
pub fn inspect(path: &Path, name: &str) -> Result<Value> {
    let (dir, record) = open(path, name)?;
    let mut stages = serde_json::Map::new();
    for phase in ["request", "response"] {
        let started = phase_started(&dir, &record, phase)?;
        stages.insert(
            phase.into(),
            if started {
                match result(&dir, &record, phase) {
                    Ok(v) => json!({"settled":true,"outcome":v["status"],"run":v["run"]}),
                    Err(e) => {
                        json!({"settled":false,"outcome":"unknown","diagnostic":e.to_string()})
                    }
                }
            } else {
                json!({"settled":false,"outcome":"not-admitted"})
            },
        );
    }
    let present = dir.join("reply.json").try_exists()?;
    let endpoint_check = current(&record);
    let revoked = dir.join("cancelled.json").try_exists()?;
    let valid = endpoint_check.is_ok() && !revoked;
    let phase = if revoked {
        "revoked"
    } else if !valid {
        "endpoint-unavailable"
    } else if stages["response"]["outcome"] != "not-admitted" {
        if stages["response"]["settled"] != true {
            "response-running-or-unconfirmed"
        } else if stages["response"]["outcome"] == "succeeded" {
            "completed"
        } else {
            "response-unsuccessful"
        }
    } else if present {
        if publication::confirmed_reply(&dir, &record).is_ok() {
            "response-ready"
        } else {
            "reply-unconfirmed"
        }
    } else if stages["request"]["outcome"] == "unknown" {
        "request-running-or-unconfirmed"
    } else if stages["request"]["settled"] == true && stages["request"]["outcome"] != "succeeded" {
        "request-unsuccessful"
    } else {
        "waiting-for-reply"
    };
    Ok(
        json!({"status":"succeeded","name":name,"request":record["approval_sha256"],"phase":phase,"endpoints_current":endpoint_check.is_ok(),
        "endpoint_diagnostic":endpoint_check.err().map(|e|e.to_string()),"admission_open":valid,
        "waiting_for_reply":phase=="waiting-for-reply","reply_record_present":present,"stages":stages,
        "waiting_process_created":false,"automatic_retries":0,"collection_authorized":false}),
    )
}

#[cfg(test)]
mod tests;
