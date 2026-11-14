//! Host-owned Cocoon transport. Provider protocols and guest claims confer no authority.
pub(crate) mod job;
pub(crate) mod transfer;
use crate::execution::{Result, store};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    os::unix::{fs::PermissionsExt, net::UnixStream},
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Profile {
    pub socket: PathBuf,
    pub grant: String,
    pub caller: String,
    pub vm: String,
    pub generation: u64,
}

pub(crate) fn load(path: &Path, digest: &str) -> Result<Profile> {
    let bytes = store::read(path, 32768)?;
    if store::digest(&bytes) != digest || fs::metadata(path)?.permissions().mode() & 0o077 != 0 {
        return Err("Cocoon profile changed or is not private".into());
    }
    let profile: Profile = serde_json::from_slice(&bytes)?;
    uuid::Uuid::parse_str(&profile.vm)?;
    if !profile.socket.is_absolute() || profile.caller.is_empty() || profile.grant.len() != 64 {
        return Err("invalid Cocoon profile".into());
    }
    Ok(profile)
}

fn exchange(profile: &Profile, id: &str, operation: Value) -> Result<Value> {
    let mut stream = UnixStream::connect(&profile.socket)?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let bytes = serde_json::to_vec(&json!({"request":id,"caller":profile.caller,
        "credential":"","grant":profile.grant,"operation":operation}))?;
    if bytes.len() > 32768 {
        return Err("Cocoon frame limit".into());
    }
    stream.write_all(&(bytes.len() as u32).to_be_bytes())?;
    stream.write_all(&bytes)?;
    let mut header = [0; 4];
    stream.read_exact(&mut header)?;
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 || length > 32768 {
        return Err("Cocoon response frame limit".into());
    }
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes)?;
    let response: Value = serde_json::from_slice(&bytes)?;
    if response["request"] != id || response["caller"] != profile.caller {
        return Err("Cocoon response identity mismatch".into());
    }
    // Never expose transport diagnostics, credentials or native replies to the model.
    if let Some(result) = response["result"].get("Ok") {
        return Ok(result.clone());
    }
    Err("Cocoon rejected operation; inspect host records".into())
}

fn validate(profile: &Profile, operation: &Value) -> Result<()> {
    match operation["operation"]
        .as_str()
        .ok_or("Cocoon operation required")?
    {
        "show" | "context" | "stop" => {
            if operation["vm"] != profile.vm {
                return Err("Cocoon VM mismatch".into());
            }
        }
        "guest" | "export" => {
            let identity = &operation["request"]["identity"];
            if identity["vm"] != profile.vm
                || identity["generation"] != profile.generation
                || identity["caller"] != profile.caller
            {
                return Err("Cocoon guest identity mismatch".into());
            }
        }
        _ => return Err("Cocoon management operation unavailable to task".into()),
    }
    Ok(())
}

fn persist(path: &Path, value: &Value) -> Result<()> {
    let parent = path.parent().ok_or("Cocoon record parent")?;
    let temporary = parent.join(format!(
        "{}.pending",
        an_agent_core::det_seam::Entropy::os().uuid_v4()
    ));
    store::write_new(&temporary, &serde_json::to_vec(value)?)?;
    fs::File::open(&temporary)?.sync_all()?;
    let published = fs::hard_link(&temporary, path);
    fs::remove_file(&temporary)?;
    published?;
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

pub(crate) fn receipts(directory: &Path) -> Result<Value> {
    if !directory.exists() {
        return Ok(json!([]));
    }
    store::no_links(directory)?;
    let mut result = Vec::new();
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|v| v.to_str()) else {
            continue;
        };
        let Some(id) = name.strip_suffix(".json") else {
            continue;
        };
        if uuid::Uuid::parse_str(id).is_err() {
            continue;
        }
        if result.len() >= 256 {
            return Err("Cocoon operation receipt limit".into());
        }
        result.push(observe(directory, id)?);
    }
    result.sort_by(|a, b| a["operation"].as_str().cmp(&b["operation"].as_str()));
    Ok(json!(result))
}

/// The durable intent is an exclusive submission fence, including after transport loss.
/// Existing intents are never sent again: only an explicit status query may reconcile.
pub(crate) fn submit(
    directory: &Path,
    profile: &Profile,
    id: &str,
    operation: Value,
) -> Result<Value> {
    uuid::Uuid::parse_str(id)?;
    validate(profile, &operation)?;
    store::directory(directory)?;
    let path = directory.join(format!("{id}.json"));
    let mut binding = json!({"vm":profile.vm,"generation":profile.generation,"caller":profile.caller,
        "operation_sha256":store::digest(&serde_json::to_vec(&operation)?)});
    if path.exists() {
        let mut original: Value = serde_json::from_slice(&store::read(&path, 32768)?)?;
        original
            .as_object_mut()
            .ok_or("Cocoon binding")?
            .remove("request");
        if original != binding {
            return Err("Cocoon operation identity already bound".into());
        }
        return observe(directory, id);
    }
    let request = an_agent_core::det_seam::Entropy::os().uuid_v4().to_string();
    binding["request"] = json!(request);
    persist(&path, &binding)?;
    fs::File::open(directory)?.sync_all()?;
    match exchange(profile, &request, operation) {
        Ok(result) => {
            persist(&directory.join(format!("{id}.result.json")), &result)?;
            fs::File::open(directory)?.sync_all()?;
            Ok(json!({"operation":id,"state":"observed","result":result,"artifact_accepted":false}))
        }
        Err(_) => Ok(json!({"operation":id,"state":"unknown","artifact_accepted":false})),
    }
}

pub(crate) fn observe(directory: &Path, id: &str) -> Result<Value> {
    uuid::Uuid::parse_str(id)?;
    store::read(&directory.join(format!("{id}.json")), 32768)?;
    let result = directory.join(format!("{id}.result.json"));
    if result.exists() {
        Ok(
            json!({"operation":id,"state":"observed","result":serde_json::from_slice::<Value>(&store::read(&result,32768)?)?,"artifact_accepted":false}),
        )
    } else {
        Ok(json!({"operation":id,"state":"unknown","artifact_accepted":false}))
    }
}

/// Query the original consumer operation; this never resubmits its side effects.
pub(crate) fn reconcile(directory: &Path, profile: &Profile, id: &str) -> Result<Value> {
    uuid::Uuid::parse_str(id)?;
    let binding: Value =
        serde_json::from_slice(&store::read(&directory.join(format!("{id}.json")), 32768)?)?;
    if binding["vm"] != profile.vm
        || binding["generation"] != profile.generation
        || binding["caller"] != profile.caller
    {
        return Err("Cocoon reconciliation binding mismatch".into());
    }
    if directory.join(format!("{id}.result.json")).exists() {
        return observe(directory, id);
    }
    let status = exchange(
        profile,
        &an_agent_core::det_seam::Entropy::os().uuid_v4().to_string(),
        json!({"operation":"operation_status","id":binding["request"]}),
    )?;
    // Retain provider status as evidence. An error or unknown is never a successful result.
    if status["status"] == "succeeded" {
        let result = &status["result"];
        let path = directory.join(format!("{id}.result.json"));
        match persist(&path, result) {
            Ok(()) => fs::File::open(directory)?.sync_all()?,
            Err(error) if path.exists() => {
                let _ = error;
            }
            Err(error) => return Err(error),
        }
    }
    observe(directory, id)
}

#[cfg(test)]
mod tests;
