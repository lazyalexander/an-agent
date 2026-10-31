//! One approved finite selection, sequential admission, and a durable no-replay fence.
use super::*;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, sync::atomic::Ordering};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    event: String,
    subscription: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    state_revision: Option<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    schema: String,
    pub(super) name: String,
    entries: Vec<Selection>,
}
impl Request {
    pub(super) fn read(path: &Path) -> Result<Self> {
        let request: Self = serde_json::from_slice(&regular(path, 65536)?)?;
        native::bundle::identifier(&request.name)?;
        if request.schema != "an-native-event-dispatch-request-v1"
            || !(1..=16).contains(&request.entries.len())
        {
            return Err("dispatch requires 1..16 explicit selections".into());
        }
        let mut seen = BTreeSet::new();
        for entry in &request.entries {
            native::bundle::identifier(&entry.subscription)?;
            if !store::valid_digest(&entry.event)
                || entry
                    .state_revision
                    .as_ref()
                    .is_some_and(|v| !store::valid_digest(v))
                || !seen.insert((&entry.event, &entry.subscription))
            {
                return Err(
                    "dispatch requires unique event/subscription pairs and exact revisions".into(),
                );
            }
        }
        Ok(request)
    }
}
fn directory(space: &Space, name: &str) -> Result<PathBuf> {
    native::bundle::identifier(name)?;
    let dir = space.root.join("dispatches").join(name);
    store::no_links(&dir)?;
    Ok(dir)
}
fn deliver(
    space: &Space,
    selected: &Selection,
    approved: Option<&str>,
    cancel: &AtomicBool,
) -> Result<Value> {
    match &selected.state_revision {
        Some(revision) => delivery::deliver_with_state(
            &space.root,
            &selected.event,
            &selected.subscription,
            revision,
            approved,
            cancel,
        ),
        None => delivery::deliver(
            &space.root,
            &selected.event,
            &selected.subscription,
            approved,
            cancel,
        ),
    }
}
pub(super) fn recorded(space: &Space, dir: &Path) -> Result<Value> {
    let value: Value =
        serde_json::from_slice(&regular(&dir.join("intent.json"), 8 * 1024 * 1024)?)?;
    if value["schema"] != "an-native-event-dispatch-intent-v1"
        || value["approval_sha256"] != hash(&value["plan"])?
        || value["plan"]["workspace"] != space.id
        || value["plan"]["request"]["name"]
            != dir
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or("dispatch name")?
    {
        return Err("dispatch intent binding mismatch".into());
    }
    Ok(value)
}

/// Observation never resumes an interrupted dispatch or grants cleanup.
pub fn inspect_dispatch(path: &Path, name: &str) -> Result<Value> {
    let space = Space::open(path)?;
    let dir = directory(&space, name)?;
    let intent = match recorded(&space, &dir) {
        Ok(value) => value,
        Err(error) if dir.exists() => {
            return Ok(json!({"schema":"an-native-event-dispatch-observation-v1",
            "status":"uncertain","reason":"incomplete-intent","diagnostic":error.to_string(),
            "executed":false,"automatic_retries":0}));
        }
        Err(error) => return Err(error),
    };
    let mut entries = Vec::new();
    let mut gap = false;
    let selected = intent["plan"]["entries"]
        .as_array()
        .ok_or("dispatch entries")?;
    for index in 0..selected.len() {
        let result = dir.join(format!("entry-{index}.json"));
        store::no_links(&result)?;
        if result.exists() {
            let value: Value = serde_json::from_slice(&regular(&result, 4 * 1024 * 1024)?)?;
            if value["dispatch_sha256"] != intent["approval_sha256"] || value["index"] != index {
                return Err("dispatch entry evidence mismatch".into());
            }
            gap |= entries.len() != index;
            entries.push(value);
        }
    }
    let summary = dir.join("result.json");
    store::no_links(&summary)?;
    let mut value = if summary.exists() {
        let value: Value = serde_json::from_slice(&regular(&summary, 65536)?)?;
        if value["schema"] != "an-native-event-dispatch-result-v1"
            || value["dispatch_sha256"] != intent["approval_sha256"]
            || value["selected_entries"] != selected.len()
        {
            return Err("dispatch result binding mismatch".into());
        }
        if gap || value["attempted_entries"] != entries.len() {
            json!({"status":"uncertain","reason":"incomplete-entry-evidence"})
        } else {
            value
        }
    } else {
        json!({"status":"uncertain","reason":"submitted-not-confirmed"})
    };
    value["schema"] = json!("an-native-event-dispatch-observation-v1");
    value["entries"] = json!(entries);
    value["executed"] = json!(false);
    value["automatic_retries"] = json!(0);
    value["authority"] =
        json!("observation only; never resume, retry, accept or reclaim from this summary");
    Ok(value)
}

pub fn dispatch(
    path: &Path,
    request: &Path,
    approved: Option<&str>,
    stop: &AtomicBool,
    cancel: &AtomicBool,
) -> Result<Value> {
    execute_request(path, Request::read(request)?, approved, stop, cancel)
}

pub(super) fn execute_request(
    path: &Path,
    request: Request,
    approved: Option<&str>,
    stop: &AtomicBool,
    cancel: &AtomicBool,
) -> Result<Value> {
    let space = Space::open(path)?;
    let dir = directory(&space, &request.name)?;
    if dir.exists() {
        let intent = recorded(&space, &dir)?;
        if intent["plan"]["request"] != serde_json::to_value(&request)? {
            return Err("dispatch name already binds another selection".into());
        }
        native::approval(&intent["plan"], approved)?;
        return inspect_dispatch(path, &request.name);
    }
    let mut entries = Vec::new();
    for selected in &request.entries {
        let preview = deliver(&space, selected, None, cancel)?;
        if preview.get("plan").is_none() || preview["executed"] != false {
            return Err(
                "dispatch selection already admitted or uncertain; reconcile explicitly".into(),
            );
        }
        entries.push(preview);
    }
    let plan = json!({"schema":"an-native-event-dispatch-plan-v1","workspace":space.id,
        "request":request,"entries":entries,"automatic_retries":0,
        "custody":"separate acceptance, state commit and completion; no automatic continuation"});
    let (digest, execute) = native::approval(&plan, approved)?;
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":digest,"executed":false}));
    }
    if stop.load(Ordering::Acquire) || cancel.load(Ordering::Acquire) {
        return Ok(
            json!({"status":if cancel.load(Ordering::Acquire) {"cancelled"} else {"succeeded"},"reason":"stopped-before-admission","executed":false}),
        );
    }
    {
        let _lock = store::lock(&space.root)?;
        let parent = space.root.join("dispatches");
        store::directory(&parent)?;
        fs::File::open(&space.root)?.sync_all()?;
        if fs::read_dir(&parent)?.take(32).count() >= 32 {
            return Err("dispatch limit reached".into());
        }
        fs::create_dir(&dir)?;
        fs::File::open(&parent)?.sync_all()?;
        write_json(
            &dir.join("intent.json"),
            &json!({"schema":"an-native-event-dispatch-intent-v1",
            "plan":plan,"approval_sha256":digest}),
        )?;
        fs::File::open(&dir)?.sync_all()?;
    }
    let mut reason = "completed";
    let mut status = "succeeded";
    let mut attempted = 0;
    for (index, selected) in request.entries.iter().enumerate() {
        // Stop finishes the selected entry; cancellation also signals its supervisor.
        if cancel.load(Ordering::Acquire) || stop.load(Ordering::Acquire) {
            reason = if cancel.load(Ordering::Acquire) {
                "cancelled"
            } else {
                "stopped"
            };
            status = if cancel.load(Ordering::Acquire) {
                "cancelled"
            } else {
                "succeeded"
            };
            break;
        }
        let result = deliver(
            &space,
            selected,
            entries[index]["approval_sha256"].as_str(),
            cancel,
        );
        attempted += 1;
        let result = match result {
            Ok(value) => value,
            Err(error) => json!({"status":"uncertain","diagnostic":error.to_string()}),
        };
        write_json(
            &dir.join(format!("entry-{index}.json")),
            &json!({"dispatch_sha256":digest,"index":index,"result":result}),
        )?;
        fs::File::open(&dir)?.sync_all()?;
        if result["status"] != "succeeded"
            || result["result"]["workspace"]["termination_confirmed"] != true
        {
            reason = "entry-not-confirmed-successful";
            status = if result["status"] == "cancelled" {
                "cancelled"
            } else {
                "uncertain"
            };
            break;
        }
    }
    let result = json!({"schema":"an-native-event-dispatch-result-v1","status":status,"reason":reason,
        "dispatch_sha256":digest,"attempted_entries":attempted,"selected_entries":request.entries.len(),
        "executed":true,"automatic_retries":0,"consumed":false});
    write_json(&dir.join("result.json"), &result)?;
    fs::File::open(&dir)?.sync_all()?;
    Ok(result)
}

#[cfg(test)]
#[path = "dispatch_tests.rs"]
mod tests;
