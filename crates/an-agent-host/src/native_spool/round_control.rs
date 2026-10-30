//! Admission control at confirmed round boundaries, never live process control.
use super::*;

const LIMIT: usize = 2 * 1024 * 1024;

fn baseline(instance: &Value) -> Value {
    json!({"schema":"an-native-spool-control-v1","instance":instance["instance"],
        "release":instance["release"],"generation":0,"mode":"active",
        "snapshot":Value::Null,"approval_sha256":Value::Null})
}

pub(super) fn current(root: &Path, instance: &Value) -> Result<Value> {
    let path = root.join("round-control.json");
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if root.join("round-control").exists()
                || instance["schema"] == "an-native-spool-instance-v2"
            {
                return Err(
                    "control head missing after installation; inspect retained evidence".into(),
                );
            }
            return Ok(baseline(instance));
        }
        Err(error) => return Err(error.into()),
        Ok(_) => (),
    }
    let head: Value = serde_json::from_slice(&regular(&path, LIMIT)?)?;
    let generation = head["generation"].as_u64().ok_or("control generation")?;
    if generation == 0 {
        if head != baseline(instance) {
            return Err("control baseline mismatch".into());
        }
        return Ok(head);
    }
    let directory = root.join("round-control");
    let journal: Value = serde_json::from_slice(&regular(
        &directory.join(format!("{generation}.json")),
        LIMIT,
    )?)?;
    let plan = &journal["plan"];
    let previous = if generation == 1 {
        baseline(instance)
    } else {
        let journal: Value = serde_json::from_slice(&regular(
            &directory.join(format!("{}.json", generation - 1)),
            LIMIT,
        )?)?;
        journal["head"].clone()
    };
    if instance["schema"] != "an-native-spool-instance-v2"
        || head["schema"] != "an-native-spool-control-v1"
        || head["instance"] != instance["instance"]
        || head["release"] != instance["release"]
        || journal["head"] != head
        || plan["schema"] != "an-native-spool-control-plan-v1"
        || plan["instance"] != instance["instance"]
        || plan["release"] != instance["release"]
        || plan["generation"] != generation
        || plan["mode"] != head["mode"]
        || plan["snapshot"] != head["snapshot"]
        || previous["generation"] != generation - 1
        || plan["previous_sha256"] != store::digest(&serde_json::to_vec(&previous)?)
        || head["approval_sha256"] != store::digest(&serde_json::to_vec(plan)?)
        || !valid_transition(&previous["mode"], &plan["operation"], &head["mode"])
    {
        return Err("control head or transition evidence mismatch".into());
    }
    Ok(head)
}

fn valid_transition(previous: &Value, operation: &Value, target: &Value) -> bool {
    matches!(
        (previous.as_str(), operation.as_str(), target.as_str()),
        (Some("active"), Some("pause"), Some("paused"))
            | (
                Some("active" | "paused"),
                Some("suspend"),
                Some("suspended")
            )
            | (Some("paused" | "suspended"), Some("resume"), Some("active"))
    )
}

pub(super) fn admitted(root: &Path, instance: &Value) -> Result<()> {
    if current(root, instance)?["mode"] != "active" {
        return Err("spool admission is paused or suspended; explicitly resume first".into());
    }
    Ok(())
}

pub(super) fn bind(plan: &mut Value, root: &Path, instance: &Value) -> Result<()> {
    let head = current(root, instance)?;
    if head["generation"] != 0 {
        plan["round_generation"] = head["generation"].clone();
    }
    Ok(())
}

pub(super) fn summary(head: &Value) -> Value {
    json!({"mode":head["mode"],"generation":head["generation"],
        "checkpoint":head["snapshot"]["state"],"process_pause":false,"automatic_execution":false})
}

fn snapshot(root: &Path, bundle: &bundle::Bundle) -> Result<Value> {
    let mut runs = Vec::new();
    for result in run::settled_runs(root)? {
        let name = result["run"].as_str().ok_or("run")?;
        let outputs = if result["status"] == "succeeded" {
            artifacts::verified(root, name, true)?.0["outputs"].clone()
        } else {
            Value::Null
        };
        runs.push(
            json!({"run":name,"result_sha256":store::digest(&serde_json::to_vec(&result)?),
            "outputs":outputs}),
        );
    }
    let checkpoint = if root.join("checkpoints").exists() {
        checkpoint::head_selection(root)?
    } else {
        Value::Null
    };
    Ok(json!({"kits":lifecycle::kit_hashes(root, bundle)?,"state":checkpoint,"runs":runs}))
}

pub fn pause(path: &Path, approved: Option<&str>) -> Result<Value> {
    transition(path, "pause", None, approved)
}
pub fn suspend(path: &Path, revision: &str, approved: Option<&str>) -> Result<Value> {
    transition(path, "suspend", Some(revision), approved)
}
pub fn resume(path: &Path, approved: Option<&str>) -> Result<Value> {
    transition(path, "resume", None, approved)
}

fn transition(
    path: &Path,
    operation: &str,
    revision: Option<&str>,
    approved: Option<&str>,
) -> Result<Value> {
    if !cfg!(target_os = "macos") {
        return Err("native round control is qualified only on macOS".into());
    }
    let root = location(path)?;
    let _lock = store::lock(&root)?;
    let (instance, bundle) = loaded(&root)?;
    let head = current(&root, &instance)?;
    let selected = snapshot(&root, &bundle)?;
    if operation == "suspend"
        && (revision.is_none() || selected["state"]["binding"]["revision"].as_str() != revision)
    {
        return Err("suspend requires the exact current committed checkpoint".into());
    }
    let mode = match operation {
        "pause" => "paused",
        "suspend" => "suspended",
        _ => "active",
    };
    if head["mode"] == mode && head["generation"] != 0 {
        if head["snapshot"] != selected {
            return Err("saved control snapshot changed".into());
        }
        let hash = head["approval_sha256"].as_str().ok_or("control approval")?;
        if approved.is_some_and(|value| value != hash) {
            return Err("control confirmation approval mismatch".into());
        }
        return Ok(json!({"status":"succeeded","round_control":summary(&head),
            "approval_sha256":hash,"executed":false,"already_recorded":true}));
    }
    if !valid_transition(&head["mode"], &json!(operation), &json!(mode)) {
        return Err("unsupported round control transition".into());
    }
    if head["mode"] != "active" && head["snapshot"] != selected {
        return Err("saved release, kit, checkpoint or completed-run evidence changed".into());
    }
    let generation = head["generation"]
        .as_u64()
        .ok_or("control generation")?
        .checked_add(1)
        .ok_or("control generation exhausted")?;
    let plan = json!({"schema":"an-native-spool-control-plan-v1","operation":operation,
        "instance":instance["instance"],"release":instance["release"],"generation":generation,
        "previous_sha256":store::digest(&serde_json::to_vec(&head)?),"mode":mode,
        "snapshot":selected,"process_control":false,"execution_granted":false});
    let (hash, execute) = approval(&plan, approved)?;
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":hash,"executed":false}));
    }
    let next = json!({"schema":"an-native-spool-control-v1","instance":instance["instance"],
        "release":instance["release"],"generation":generation,"mode":mode,
        "snapshot":selected,"approval_sha256":hash});
    let journal = json!({"head":next,"plan":plan});
    let bytes = serde_json::to_vec_pretty(&journal)?;
    if bytes.len() > LIMIT {
        return Err("control evidence exceeds limit".into());
    }
    // Install legacy generation zero before any journal can become orphaned.
    let target = root.join("round-control.json");
    if !target.exists() {
        write_json(&target, &head)?;
    }
    let directory = root.join("round-control");
    store::directory(&directory)?;
    // Older hosts must refuse a controlled instance instead of ignoring its
    // admission head. Upgrade before publishing any paused/suspended mode.
    if instance["schema"] == "an-native-spool-instance-v1" {
        let mut upgraded = instance.clone();
        upgraded["schema"] = json!("an-native-spool-instance-v2");
        let bytes = serde_json::to_vec_pretty(&upgraded)?;
        let pending = directory.join("instance-v2.pending");
        if pending.exists() {
            if regular(&pending, 65536)? != bytes {
                return Err("conflicting instance control upgrade".into());
            }
        } else {
            store::write_new(&pending, &bytes)?;
        }
        fs::rename(pending, root.join("instance.json"))?;
    }
    let intent = directory.join(format!("{generation}.json"));
    if intent.exists() {
        if regular(&intent, LIMIT)? != bytes {
            return Err("conflicting control publication; inspect retained journal".into());
        }
    } else {
        store::write_new(&intent, &bytes)?;
    }
    let bytes = serde_json::to_vec_pretty(&next)?;
    let pending = directory.join(format!("{generation}-{hash}.pending"));
    if pending.exists() {
        if regular(&pending, LIMIT)? != bytes {
            return Err("conflicting control head staging".into());
        }
    } else {
        store::write_new(&pending, &bytes)?;
    }
    fs::rename(pending, target)?;
    Ok(json!({"status":"succeeded","round_control":summary(&next),
        "executed":true,"already_recorded":false}))
}
