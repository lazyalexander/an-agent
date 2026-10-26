//! Optional file-based proposal slot. Adapters receive data, never host authority.
use super::*;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Proposal {
    schema: String,
    request: String,
    request_sha256: String,
    kit: String,
    content: String,
}

fn epoch(root: &Path) -> Result<String> {
    let mut names = Vec::new();
    for entry in fs::read_dir(root.join("runs"))? {
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "invalid run name")?;
        names.push(format!("run/{name}"));
        // A request made after preview must still expire when that run starts.
        for marker in [
            "start.json",
            "result.json",
            "accepted.json",
            "reclaimed.json",
        ] {
            if entry.path().join(marker).exists() {
                names.push(format!("run/{name}/{marker}"));
            }
        }
    }
    // Include edit identities, not only bytes: edit-away/edit-back is a new generation.
    for entry in fs::read_dir(root)? {
        let name = entry?.file_name();
        if let Some(name) = name.to_str().filter(|n| n.starts_with("edit-")) {
            names.push(format!("edit/{name}"));
        }
    }
    names.sort();
    Ok(store::digest(&serde_json::to_vec(&names)?))
}

/// Select one editable kit for an external adapter; no adapter is launched.
pub fn request(path: &Path, name: &str) -> Result<Value> {
    bundle::identifier(name)?;
    let root = location(path)?;
    let _lock = store::lock(&root)?;
    let (instance, bundle) = active(&root)?;
    let kit = format!("kit/{name}");
    if !bundle.payload.contains_key(&kit) {
        return Err("weaver requires a declared editable kit".into());
    }
    let dir = root.join("weaver");
    store::directory(&dir)?;
    if fs::read_dir(&dir)?.take(128).count() >= 128 {
        return Err("weaver request limit reached; load a fresh instance".into());
    }
    let code = regular(&root.join(&kit), 1024 * 1024)?;
    let id = an_agent_core::det_seam::Entropy::os().uuid_v4().to_string();
    let value = json!({"schema":"an-weaver-request-v1","request":id,
        "instance":instance["instance"],"release":instance["release"],"kit":kit,
        "before":store::digest(&code),"epoch":epoch(&root)?,
        "content":String::from_utf8(code)?,"execution_granted":false});
    let bytes = serde_json::to_vec_pretty(&value)?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("encoded weaver request exceeds the exchange limit".into());
    }
    store::write_new(&dir.join(format!("{id}.json")), &bytes)?;
    Ok(json!({"request":value,"request_sha256":store::digest(&bytes)}))
}

fn ticket(root: &Path, id: &str) -> Result<(Value, Vec<u8>, PathBuf)> {
    uuid::Uuid::parse_str(id)?;
    let dir = root.join("weaver");
    let done = dir.join(format!("{id}.closed.json"));
    if done.exists() {
        return Err("weaver request is closed; no retry or late publication".into());
    }
    let bytes = regular(&dir.join(format!("{id}.json")), 2 * 1024 * 1024)?;
    Ok((serde_json::from_slice(&bytes)?, bytes, done))
}

/// Revoke result admission. This does not claim an external agent has terminated.
pub fn revoke(path: &Path, id: &str) -> Result<Value> {
    let root = location(path)?;
    let _lock = store::lock(&root)?;
    active(&root)?;
    let (_, _, done) = ticket(&root, id)?;
    let value = json!({"status":"succeeded","request":id,"result_admission":"revoked",
        "external_termination_confirmed":false});
    write_json(&done, &value)?;
    Ok(value)
}

/// Import a bounded proposal under the same lock and edit authority as operator edits.
pub fn apply(path: &Path, proposal: &Path, approved: Option<&str>) -> Result<Value> {
    let bytes = regular(proposal, 2 * 1024 * 1024)?;
    let p: Proposal = serde_json::from_slice(&bytes)?;
    if p.schema != "an-weaver-proposal-v1" || p.content.len() > 1024 * 1024 {
        return Err("invalid weaver proposal schema or size".into());
    }
    let root = location(path)?;
    let _lock = store::lock(&root)?;
    let (instance, bundle) = active(&root)?;
    let (request, request_bytes, done) = ticket(&root, &p.request)?;
    if request["instance"] != instance["instance"]
        || request["release"] != instance["release"]
        || request["epoch"] != epoch(&root)?
        || request["kit"] != p.kit
        || !p.kit.starts_with("kit/")
        || !bundle.payload.contains_key(&p.kit)
        || store::digest(&request_bytes) != p.request_sha256
        || request["before"] != store::digest(&regular(&root.join(&p.kit), 1024 * 1024)?)
    {
        return Err("stale or out-of-scope weaver proposal".into());
    }
    let plan = json!({"request":p.request,"request_sha256":p.request_sha256,
        "instance":instance["instance"],"release":instance["release"],"kit":p.kit,
        "before":request["before"],"after":store::digest(p.content.as_bytes()),
        "execution_granted":false});
    let (hash, execute) = approval(&plan, approved)?;
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":hash,"executed":false}));
    }
    // Close before mutation: an interrupted import is inspectable, never replayed.
    write_json(&done, &json!({"status":"commit-intent","plan":plan}))?;
    lifecycle::commit_kit(&root, &p.kit, p.content.as_bytes(), &plan)?;
    Ok(json!({"status":"succeeded","edit":plan}))
}
