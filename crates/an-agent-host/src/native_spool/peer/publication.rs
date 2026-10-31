//! Short metadata commits; no endpoint lock is held while a peer executes.
use super::*;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    source_run: String,
    source_output: String,
    target: PathBuf,
    reply_output: String,
    wall_seconds: u64,
    request_state: Option<String>,
    response_state: Option<String>,
}

pub fn request(path: &Path, name: &str, input: &Path, approved: Option<&str>) -> Result<Value> {
    bundle::identifier(name)?;
    let spec: Request = serde_json::from_slice(&regular(input, 65536)?)?;
    if spec.schema != "an-native-peer-request-v1" || !(1..=300).contains(&spec.wall_seconds) {
        return Err("unsupported peer request".into());
    }
    for revision in [&spec.request_state, &spec.response_state]
        .into_iter()
        .flatten()
    {
        if !store::valid_digest(revision) {
            return Err("peer state must be an exact revision".into());
        }
    }
    let root = location(path)?;
    let target = location(&spec.target)?;
    if root.starts_with(&target) || target.starts_with(&root) {
        return Err("peer instances must be separate nonnested directories".into());
    }
    // Fail-fast locks cannot deadlock on a reciprocal request.
    let _source = store::lock(&root)?;
    let _target = store::lock(&target)?;
    let source = endpoint(&root)?;
    let target_id = endpoint(&target)?;
    let (_, target_bundle) = active(&target)?;
    if !target_bundle.manifest.outputs.contains(&spec.reply_output) {
        return Err("peer reply must be a declared target output".into());
    }
    // A wait starts only after the requesting computation has stopped.
    if run::settled_runs(&root)?
        .iter()
        .any(|r| r["workspace"]["termination_confirmed"] != true)
    {
        return Err("peer wait requires confirmed requester termination".into());
    }
    let data = artifact(&root, &spec.source_run, &spec.source_output)?;
    let parent = root.join("peers");
    store::no_links(&parent)?;
    let dir = parent.join(name);
    if dir.try_exists()? {
        return Err("peer identity already used; inspect, never replay".into());
    }
    if parent.exists() && fs::read_dir(&parent)?.take(64).count() >= 64 {
        return Err("peer request limit reached".into());
    }
    let plan = json!({"schema":spec.schema,"name":name,"source":source,"target":target_id,
        "source_run":spec.source_run,"source_output":spec.source_output,"data":data,
        "reply_output":spec.reply_output,"wall_seconds":spec.wall_seconds,
        "request_state":spec.request_state,"response_state":spec.response_state});
    let (digest, execute) = approval(&plan, approved)?;
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":digest,"executed":false}));
    }
    store::directory(&parent)?;
    fs::create_dir(&dir)?;
    write_json(
        &dir.join("request.json"),
        &json!({"plan":plan,"approval_sha256":digest}),
    )?;
    sync(&dir)?;
    sync(&parent)?;
    sync(&root)?;
    Ok(
        json!({"status":"succeeded","request":digest,"waiting_for_reply":true,"process_started":false}),
    )
}

pub(super) fn reply_plan(dir: &Path, record: &Value) -> Result<Value> {
    let target = Path::new(text(&record["plan"]["target"], "root")?);
    let settled = result(dir, record, "request")?;
    let data = artifact(
        target,
        text(&settled, "run")?,
        text(&record["plan"], "reply_output")?,
    )?;
    let plan = json!({"operation":"peer-reply","request":record["approval_sha256"],
        "context":context(record,"response"),"result_sha256":hash(&settled)?,"data":data});
    Ok(plan)
}

pub(super) fn confirmed_reply(dir: &Path, record: &Value) -> Result<Value> {
    let plan = reply_plan(dir, record)?;
    let receipt = read(&dir.join("reply.json"))?;
    if receipt != json!({"plan":plan,"approval_sha256":hash(&plan)?}) {
        return Err("peer response does not match original execution and material".into());
    }
    Ok(plan["data"].clone())
}

pub fn reply(path: &Path, name: &str, approved: Option<&str>) -> Result<Value> {
    let (dir, record) = open(path, name)?;
    let source = Path::new(text(&record["plan"]["source"], "root")?);
    let target = Path::new(text(&record["plan"]["target"], "root")?);
    let _source = store::lock(source)?;
    let _target = store::lock(target)?;
    let _request = store::lock(&dir)?;
    available(&dir, &record)?;
    let plan = reply_plan(&dir, &record)?;
    let (digest, execute) = approval(&plan, approved)?;
    let value = json!({"plan":plan,"approval_sha256":digest});
    let destination = dir.join("reply.json");
    if destination.exists() {
        if read(&destination)? != value {
            return Err("peer reply identity changed".into());
        }
        return Ok(json!({"status":"succeeded","already_recorded":true,"reply":value}));
    }
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":digest,"executed":false}));
    }
    write_json(&destination, &value)?;
    sync(&dir)?;
    Ok(json!({"status":"succeeded","reply":value,"process_started":false}))
}

/// Revoke future admission and reply consumption; do not signal an independent peer.
pub fn cancel(path: &Path, name: &str, approved: Option<&str>) -> Result<Value> {
    let (dir, record) = open(path, name)?;
    let _lock = store::lock(&dir)?;
    let plan = json!({"operation":"peer-cancel","request":record["approval_sha256"],
        "signals_peer":false,"deletes_artifacts":false});
    let (digest, execute) = approval(&plan, approved)?;
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":digest,"executed":false}));
    }
    let value = json!({"plan":plan,"approval_sha256":digest});
    let destination = dir.join("cancelled.json");
    if destination.exists() {
        if read(&destination)? != value {
            return Err("peer cancellation evidence changed".into());
        }
    } else {
        write_json(&destination, &value)?;
    }
    sync(&dir)?;
    Ok(
        json!({"status":"succeeded","admission_open":false,"signals_peer":false,"collection_authorized":false}),
    )
}
