//! Fixed-code start plans and instance-bound custody; domain sequencing stays in silk.
use super::inputs::Input;
use super::*;
use crate::workspace_job;
use std::sync::atomic::AtomicBool;

fn profile(
    root: &Path,
    bundle: &bundle::Bundle,
    selected: &inputs::Selection,
    seconds: u64,
) -> Result<Value> {
    if !(1..=300).contains(&seconds) {
        return Err("invalid run deadline".into());
    }
    let mut inputs = Vec::new();
    let mut entry = None;
    for (index, path) in bundle.payload.keys().enumerate() {
        let source = if path.starts_with("kit/") {
            root.join(path)
        } else {
            root.join("release").join(path)
        };
        inputs.push(source);
        if *path == bundle.manifest.entry {
            entry = Some(index);
        }
    }
    let args = vec![format!("{{input:{}}}", entry.ok_or("missing silk entry")?)];
    let mut profile = json!({"schema":"an-workspace-job-v2","executable":"/bin/bash","args":args,
        "environment":{"AN_SPOOL_SILK_DIR":"{inputs}","AN_SPOOL_KIT_DIR":"{inputs}"},
        "readonly":[],"inputs":inputs,"outputs":bundle.manifest.outputs,
        "wall_seconds":seconds,"trusted_process_group":true});
    if !bundle.manifest.binary_outputs.is_empty() {
        profile["binary_outputs"] = json!(bundle.manifest.binary_outputs);
    }
    selected.configure(bundle, &mut profile)?;
    runtime::profile(root, bundle, &mut profile)?;
    Ok(profile)
}

pub fn start(
    path: &Path,
    name: &str,
    input: &Path,
    seconds: u64,
    approved: Option<&str>,
    cancelled: &AtomicBool,
) -> Result<Value> {
    start_selected(
        path,
        name,
        Input::File(input),
        seconds,
        approved,
        RunAdmission {
            cancelled,
            supervisor: None,
        },
        |_| Ok(()),
    )
}

pub fn start_state(
    path: &Path,
    name: &str,
    revision: &str,
    seconds: u64,
    approved: Option<&str>,
    cancelled: &AtomicBool,
) -> Result<Value> {
    start_selected(
        path,
        name,
        Input::State(revision),
        seconds,
        approved,
        RunAdmission {
            cancelled,
            supervisor: None,
        },
        |_| Ok(()),
    )
}

/// Bind one accepted business artifact and one explicitly selected checkpoint.
pub fn start_data_state(
    path: &Path,
    name: &str,
    selection: &Path,
    seconds: u64,
    approved: Option<&str>,
    cancelled: &AtomicBool,
) -> Result<Value> {
    start_selected(
        path,
        name,
        Input::DataState(selection),
        seconds,
        approved,
        RunAdmission {
            cancelled,
            supervisor: None,
        },
        |_| Ok(()),
    )
}

/// The host delivery adapter checks its grant and records intent at start admission.
pub(super) fn start_bound(
    path: &Path,
    name: &str,
    selected: inputs::BoundInput<'_>,
    seconds: u64,
    approved: Option<&str>,
    cancelled: &AtomicBool,
    before_start: impl FnOnce(&Value) -> Result<()>,
) -> Result<Value> {
    start_selected(
        path,
        name,
        Input::Bound(selected),
        seconds,
        approved,
        RunAdmission {
            cancelled,
            supervisor: None,
        },
        before_start,
    )
}

struct RunAdmission<'a> {
    cancelled: &'a AtomicBool,
    supervisor: Option<&'a str>,
}

pub(super) fn start_supervised(
    root: &Path,
    name: &str,
    input: &Path,
    seconds: u64,
    request: &Value,
    cancelled: &AtomicBool,
) -> Result<Value> {
    let start = &request["plan"]["start"];
    let approval = store::digest(&serde_json::to_vec(start)?);
    start_selected(
        root,
        name,
        Input::File(input),
        seconds,
        Some(&approval),
        RunAdmission {
            cancelled,
            supervisor: request["approval_sha256"].as_str(),
        },
        |observed| {
            if observed != start {
                return Err("supervisor native plan changed".into());
            }
            Ok(())
        },
    )
}

fn start_selected(
    path: &Path,
    name: &str,
    input: Input<'_>,
    seconds: u64,
    approved: Option<&str>,
    admission: RunAdmission<'_>,
    before_start: impl FnOnce(&Value) -> Result<()>,
) -> Result<Value> {
    let cancelled = admission.cancelled;
    bundle::identifier(name)?;
    let root = location(path)?;
    let _lock = store::lock(&root)?;
    let (instance, bundle) = active(&root)?;
    super::supervisor::admit(&root, name, admission.supervisor)?;
    let dir = root.join("runs").join(name);
    if !dir.exists() && fs::read_dir(root.join("runs"))?.take(64).count() >= 64 {
        return Err("instance run limit reached; load a fresh instance".into());
    }
    if dir.join("job").exists() {
        return Err("run identity already used; no implicit retry".into());
    }
    let selected = inputs::select(&root, &bundle, input)?;
    let profile = profile(&root, &bundle, &selected, seconds)?;
    let bytes = serde_json::to_vec_pretty(&profile)?;
    // Preview may persist a bounded plan, but never launches or grants execution.
    store::directory(&dir)?;
    let owner = json!({"instance":instance["instance"],"release":bundle.release,"run":name});
    let owner_path = dir.join("spool-owner.json");
    if owner_path.exists() {
        let prior: Value = serde_json::from_slice(&regular(&owner_path, 65536)?)?;
        if prior != owner {
            return Err("run owner changed".into());
        }
    } else {
        write_json(&owner_path, &owner)?;
    }
    let profile_path = dir.join("profile.json");
    if profile_path.exists() {
        if regular(&profile_path, 65536)? != bytes {
            return Err("run name already binds another profile".into());
        }
    } else {
        store::write_new(&profile_path, &bytes)?;
    }
    let preview =
        workspace_job::run_managed(&profile_path, &dir.join("job"), None, None, cancelled)?;
    let mut plan = json!({"schema":"an-native-spool-start-v1","instance":instance["instance"],
        "release":bundle.release,"run":name,"workspace":preview["plan"],
        "authority":"reviewed native tools only; no detached workers or network; no hostile-agent claim"});
    selected.bind_plan(&mut plan);
    round_control::bind(&mut plan, &root, &instance)?;
    let (hash, execute) = approval(&plan, approved)?;
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":hash,"executed":false}));
    }
    if cancelled.load(std::sync::atomic::Ordering::Acquire) {
        return Ok(json!({"status":"cancelled","executed":false,"process_started":false}));
    }
    before_start(&plan)?;
    super::input_references::acquire(&root, name, &plan)?;
    write_json(&dir.join("start.json"), &plan)?;
    let receipt = workspace_job::run_managed(
        &profile_path,
        &dir.join("job"),
        None,
        preview["approval_sha256"].as_str(),
        cancelled,
    )?;
    let result = json!({"schema":"an-native-spool-run-v1","instance":instance["instance"],
        "release":bundle.release,"run":name,"profile_sha256":store::digest(&bytes),
        "status":receipt["status"],"workspace":receipt});
    write_json(&dir.join("result.json"), &result)?;
    Ok(result)
}

pub(super) fn settled(root: &Path, name: &str) -> Result<Value> {
    bundle::identifier(name)?;
    let (instance, _) = state(root)?;
    let dir = root.join("runs").join(name);
    let result: Value =
        serde_json::from_slice(&regular(&dir.join("result.json"), 2 * 1024 * 1024)?)?;
    let id = result["workspace"]["attempt"]
        .as_str()
        .ok_or("missing started attempt")?;
    let job = dir.join("job");
    let (_, receipt) = if job.join("reclaimed.json").exists() {
        workspace_job::archived_record(&job, id)?
    } else {
        workspace_job::record(&job, id)?
    };
    let plan: Value = serde_json::from_slice(&regular(
        &dir.join("job/attempts").join(id).join("plan.json"),
        2 * 1024 * 1024,
    )?)?;
    if result["schema"] != "an-native-spool-run-v1"
        || result["instance"] != instance["instance"]
        || result["release"] != instance["release"]
        || result["run"] != name
        || result["workspace"] != receipt
        || result["profile_sha256"] != plan["profile_sha256"]
        || result["profile_sha256"] != store::digest(&regular(&dir.join("profile.json"), 65536)?)
    {
        return Err("spool run ownership or receipt mismatch".into());
    }
    Ok(result)
}

/// A round boundary requires every started job to have confirmed termination.
pub(super) fn settled_runs(root: &Path) -> Result<Vec<Value>> {
    let mut runs = Vec::new();
    for entry in fs::read_dir(root.join("runs"))? {
        let path = entry?.path();
        store::no_links(&path)?;
        if path.join("job").exists() {
            runs.push(settled(
                root,
                path.file_name()
                    .and_then(|s| s.to_str())
                    .ok_or("invalid run")?,
            )?);
        } else if path.join("start.json").exists()
            || path.join("result.json").exists()
            || path.join("supervisor").exists()
        {
            return Err("started spool run lacks workspace settlement evidence".into());
        }
    }
    runs.sort_by_key(|r| r["run"].as_str().unwrap_or_default().to_owned());
    Ok(runs)
}

pub fn accept(path: &Path, name: &str, approved: Option<&str>) -> Result<Value> {
    custody(path, name, approved, false)
}
pub fn reclaim(path: &Path, name: &str, approved: Option<&str>) -> Result<Value> {
    custody(path, name, approved, true)
}
fn custody(path: &Path, name: &str, approved: Option<&str>, collect: bool) -> Result<Value> {
    let root = location(path)?;
    let _lock = store::lock(&root)?;
    let result = settled(&root, name)?;
    if !collect {
        let (instance, bundle) = state(&root)?;
        runtime::verify(&root, &instance, &bundle)?;
    }
    let job = root.join("runs").join(name).join("job");
    let id = result["workspace"]["attempt"]
        .as_str()
        .ok_or("missing attempt")?;
    let operation = if collect {
        workspace_job::reclaim_managed
    } else {
        workspace_job::accept_managed
    };
    let preview = operation(&job, id, None)?;
    let plan = json!({"instance":result["instance"],"release":result["release"],"run":name,
        "operation":if collect {"reclaim"} else {"accept"},"workspace":preview["plan"]});
    let (hash, execute) = approval(&plan, approved)?;
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":hash,"executed":false}));
    }
    let accepted = operation(&job, id, preview["approval_sha256"].as_str())?;
    let bound = json!({"status":"succeeded","instance":result["instance"],"release":result["release"],
        "run":name,"operation":plan["operation"],"result":accepted});
    write_json(
        &root.join("runs").join(name).join(if collect {
            "reclaimed.json"
        } else {
            "accepted.json"
        }),
        &bound,
    )?;
    Ok(bound)
}
