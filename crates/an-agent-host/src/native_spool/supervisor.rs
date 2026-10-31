//! One approved native attempt owned by a separate, finite supervisor process.
//! Host-private records authorize control; process observations grant no custody.
use super::*;
use std::sync::atomic::AtomicBool;
mod worker;
pub use worker::execute;

const LIMIT: usize = 2 * 1024 * 1024;

fn read(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&regular(path, LIMIT)?)?)
}
fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("missing supervisor {key}").into())
}
fn sync(path: &Path) -> Result<()> {
    fs::File::open(path)?.sync_all()?;
    Ok(())
}
pub fn directory(root: &Path, name: &str) -> Result<PathBuf> {
    bundle::identifier(name)?;
    let path = location(root)?.join("runs").join(name).join("supervisor");
    store::no_links(&path)?;
    Ok(path)
}

fn request(root: &Path, name: &str) -> Result<(PathBuf, Value)> {
    let dir = directory(root, name)?;
    let record = read(&dir.join("request.json"))?;
    let plan = &record["plan"];
    let (instance, _) = state(&location(root)?)?;
    if plan["schema"] != "an-native-supervisor-v1"
        || plan["root"] != json!(location(root)?)
        || plan["run"] != name
        || plan["start"]["instance"] != instance["instance"]
        || plan["start"]["release"] != instance["release"]
        || record["approval_sha256"] != store::digest(&serde_json::to_vec(plan)?)
    {
        return Err("supervisor request identity mismatch".into());
    }
    Ok((dir, record))
}

pub enum Input<'a> {
    File(&'a Path),
    State(&'a str),
}
pub fn prepare(
    root: &Path,
    name: &str,
    input: Input<'_>,
    seconds: u64,
    executable: &Path,
    approved: Option<&str>,
) -> Result<Value> {
    let root = location(root)?;
    let (input, revision, preview) = match input {
        Input::File(path) => {
            let path = location(path)?;
            let preview = run::start(&root, name, &path, seconds, None, &AtomicBool::new(false))?;
            (json!(path), None, preview)
        }
        Input::State(revision) => {
            if !store::valid_digest(revision) {
                return Err("state requires an exact revision".into());
            }
            let preview = run::start_state(
                &root,
                name,
                revision,
                seconds,
                None,
                &AtomicBool::new(false),
            )?;
            (Value::Null, Some(revision), preview)
        }
    };
    let executable = location(executable)?;

    let mut plan = json!({"schema":"an-native-supervisor-v1","root":root,"run":name,
        "input":input,"seconds":seconds,"executable":executable,"start":preview["plan"]});
    if let Some(revision) = revision {
        plan["state_revision"] = json!(revision);
    }
    let (hash, execute) = approval(&plan, approved)?;
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":hash,"reserved":false,"executed":false}));
    }
    let _lock = store::lock(&root)?;
    let (instance, _) = active(&root)?;
    let generation = round_control::current(&root, &instance)?["generation"].clone();
    if plan["start"]["instance"] != instance["instance"]
        || plan["start"]["release"] != instance["release"]
        || plan["start"]["round_generation"].as_u64().unwrap_or(0)
            != generation.as_u64().ok_or("generation")?
    {
        return Err("supervisor activation changed before reservation".into());
    }
    let run_dir = root.join("runs").join(name);
    if run_dir.join("job").try_exists()? || run_dir.join("start.json").try_exists()? {
        return Err("supervisor run already admitted; inspect, never replay".into());
    }
    let dir = directory(&root, name)?;
    // Directory creation is the single-use launch reservation. A torn write
    // leaves the identity fenced rather than authorizing another launch.
    fs::create_dir(&dir)?;
    write_json(
        &dir.join("request.json"),
        &json!({"plan":plan,"approval_sha256":hash}),
    )?;
    sync(&dir)?;
    sync(&run_dir)?;
    Ok(json!({"plan":plan,"approval_sha256":hash,"reserved":true,"executed":false}))
}

/// Direct or legacy entrypoints cannot consume a reserved supervised run.
pub(super) fn admit(root: &Path, name: &str, permit: Option<&str>) -> Result<()> {
    let dir = directory(root, name)?;
    if !dir.try_exists()? && permit.is_none() {
        return Ok(());
    }
    let (dir, record) = request(root, name)?;
    if permit != record["approval_sha256"].as_str()
        || read(&dir.join("owner.json"))?["request"] != record["approval_sha256"]
        || dir.join("completion.json").try_exists()?
    {
        return Err("run reserved for original supervisor; inspect, never replay".into());
    }
    Ok(())
}

pub fn cancel(root: &Path, name: &str, approved: Option<&str>) -> Result<Value> {
    let (dir, record) = request(root, name)?;
    let _lock = store::lock(&dir)?;
    let plan = json!({"operation":"cancel-native-supervisor","request":record["approval_sha256"],
        "instance":record["plan"]["start"]["instance"],"run":name});
    let (hash, execute) = approval(&plan, approved)?;
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":hash,"executed":false}));
    }
    let cancellation = json!({"request":record["approval_sha256"]});
    let path = dir.join("cancel.json");
    if path.try_exists()? {
        if read(&path)? != cancellation {
            return Err("supervisor cancellation binding mismatch".into());
        }
    } else {
        write_json(&path, &cancellation)?;
    }
    sync(&dir)?;
    Ok(json!({"status":"succeeded","cancellation_requested":true,
        "termination_confirmed":false,"request":record["approval_sha256"]}))
}

pub fn inspect(root: &Path, name: &str) -> Result<Value> {
    let (dir, record) = request(root, name)?;
    let native = super::status(root, name)?;
    Ok(
        json!({"status":"succeeded","request":record["approval_sha256"],"run":name,
        "owner_record_present":dir.join("owner.json").try_exists()?,
        "completion_record_present":dir.join("completion.json").try_exists()?,
        "cancellation_requested":dir.join("cancel.json").try_exists()?,
        "native":native,"automatic_retries":0}),
    )
}

/// A client reads only the final response bound to its one launch request.
pub fn completion(root: &Path, name: &str, hash: &str) -> Result<Value> {
    let (dir, record) = request(root, name)?;
    let completion = read(&dir.join("completion.json"))?;
    if record["approval_sha256"] != hash || completion["request"] != hash {
        return Err("supervisor completion identity mismatch".into());
    }
    if let Some(error) = completion["error"].as_str() {
        return Err(format!("supervisor failed; inspect original evidence: {error}").into());
    }
    let result = completion["result"].clone();
    if result.get("workspace").is_some() {
        if run::settled(root, name)? != result {
            return Err("supervisor result does not match native settlement".into());
        }
    } else if result != json!({"status":"cancelled","executed":false,"process_started":false}) {
        return Err("supervisor response lacks execution evidence".into());
    }
    Ok(result)
}
