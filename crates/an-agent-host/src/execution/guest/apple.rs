//! Kit: Apple Container observations never grant permission or accept artifacts.
mod transport;
use crate::execution::{Result, store};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Profile {
    pub cli: PathBuf,
    pub cli_sha256: String,
    pub ledger: PathBuf,
    pub image: String,
    pub attempt: String,
    pub created_date: String,
    pub started_date: String,
    pub expires_unix: u64,
}

impl Profile {
    pub(crate) fn validate(&self) -> Result<()> {
        if !cfg!(target_os = "macos")
            || !self.cli.is_absolute()
            || !self.ledger.is_absolute()
            || !store::valid_digest(&self.cli_sha256)
            || !self
                .image
                .rsplit_once("@sha256:")
                .is_some_and(|(name, hash)| {
                    !name.is_empty()
                        && !name.starts_with('-')
                        && !name.chars().any(char::is_whitespace)
                        && store::valid_digest(hash)
                })
        {
            return Err("invalid Apple Container operator profile; no native fallback".into());
        }
        uuid::Uuid::parse_str(&self.attempt)?;
        store::no_links(&self.ledger)?;
        if std::fs::metadata(&self.cli)?.len() > 128 * 1024 * 1024
            || crate::execution::sandbox::service_process::executable_hash(&self.cli)?
                != self.cli_sha256
        {
            return Err("Apple Container executable changed".into());
        }
        Ok(())
    }

    fn active(&self) -> Result<()> {
        if self.ledger.join("revoked.json").exists()
            || an_agent_core::det_seam::Clock::wall().now_ms() / 1000 >= self.expires_unix
        {
            return Err("Apple Container work authority expired or revoked".into());
        }
        Ok(())
    }
}

/// Identity is checked against trusted runtime metadata, never guest output.
fn owned(value: &Value, profile: &crate::cocoon::Profile, apple: &Profile) -> Result<Value> {
    let entries = value.as_array().ok_or("invalid container inspection")?;
    if entries.len() != 1 {
        return Err("container inspection cardinality".into());
    }
    let entry = &entries[0];
    let config = &entry["configuration"];
    if entry["id"] != profile.vm
        || config["id"] != profile.vm
        || config["labels"]["an-agent.attempt"] != apple.attempt
        || config["labels"]["an-agent.generation"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            != Some(profile.generation)
        || config["creationDate"] != apple.created_date
        || (!entry["status"]["startedDate"].is_null()
            && entry["status"]["startedDate"] != apple.started_date)
        || config["image"]["descriptor"]["digest"]
            != format!(
                "sha256:{}",
                apple.image.rsplit_once("@sha256:").ok_or("image digest")?.1
            )
        || config["platform"]["os"] != "linux"
        || config["platform"]["architecture"] != "arm64"
        || config["ssh"] != false
        || config["virtualization"] != false
    {
        return Err("Apple Container owner or configuration mismatch".into());
    }
    for field in ["networks", "mounts", "publishedPorts", "publishedSockets"] {
        if !config[field].as_array().is_some_and(Vec::is_empty) {
            return Err("Apple Container must have no network or host attachment".into());
        }
    }
    Ok(entry.clone())
}

pub(crate) fn exchange(
    profile: &crate::cocoon::Profile,
    id: &str,
    operation: Value,
) -> Result<Value> {
    let apple = profile
        .apple_container
        .as_ref()
        .ok_or("Apple Container profile missing")?;
    apple.validate()?;
    uuid::Uuid::parse_str(id)?;
    let kind = operation["operation"].as_str().ok_or("operation missing")?;
    if kind == "operation_status" {
        let original = operation["id"]
            .as_str()
            .ok_or("original operation missing")?;
        uuid::Uuid::parse_str(original)?;
        let path = apple.ledger.join(format!("{original}.result.json"));
        return if path.exists() {
            Ok(
                json!({"status":"succeeded", "result":serde_json::from_slice::<Value>(&store::read(&path,32768)?)?}),
            )
        } else {
            Ok(json!({"status":"unknown","automatic_retries":0}))
        };
    }
    let observed = transport::call(apple, &["inspect", &profile.vm], 10)?;
    let state = owned(&serde_json::from_slice(&observed)?, profile, apple)?;
    let running = state["status"]["state"] == "running";
    if matches!(kind, "context" | "show") {
        let active = apple.active().is_ok();
        return Ok(
            json!({"id":profile.vm,"vm":profile.vm,"generation":profile.generation,
            "attempt":apple.attempt,"active_attempt":apple.attempt,
            "active_lease":if running {json!(apple.attempt)} else {Value::Null},
            "state":state["status"]["state"],"runtime":"apple-container",
            "work":{"guest_ready":running && active,"actions":if active {json!(["inspect","guest","stop"])} else {json!(["inspect","stop"])}}}),
        );
    }
    if !matches!(kind, "guest" | "stop") {
        return Err("unsupported Apple Container operation".into());
    }
    if kind == "guest" {
        apple.active()?;
        if !running || operation["request"]["identity"]["attempt"] != apple.attempt {
            return Err("Apple Container attempt is not active".into());
        }
    }
    let intent = apple.ledger.join(format!("{id}.json"));
    let result = apple.ledger.join(format!("{id}.result.json"));
    if intent.exists() {
        if serde_json::from_slice::<Value>(&store::read(&intent, 32768)?)? != operation {
            return Err("Apple Container operation already bound".into());
        }
        return serde_json::from_slice(&store::read(&result, 32768)?).map_err(Into::into);
    }
    // A durable exclusive fence precedes every mutation. Missing results stay unknown.
    store::write_new(&intent, &serde_json::to_vec(&operation)?)?;
    std::fs::File::open(&apple.ledger)?.sync_all()?;
    let value = if kind == "stop" {
        transport::call(apple, &["stop", "--time", "1", &profile.vm], 15)?;
        let observed = transport::call(apple, &["inspect", &profile.vm], 10)?;
        let state = owned(&serde_json::from_slice(&observed)?, profile, apple)?;
        if state["status"]["state"] != "stopped" {
            return Err("container termination unconfirmed".into());
        }
        json!({"termination_confirmed":true})
    } else {
        apple.active()?;
        let input = serde_json::to_vec(&operation["request"])?;
        let bytes = transport::call_with_input(
            apple,
            &[
                "exec",
                "-i",
                &profile.vm,
                "/usr/local/bin/python3",
                "-I",
                "-c",
                include_str!("apple/guest.py"),
            ],
            &input,
            30,
        )?;
        serde_json::from_slice::<Value>(&bytes)?
    };
    store::write_new(&result, &serde_json::to_vec(&value)?)?;
    std::fs::File::open(&apple.ledger)?.sync_all()?;
    Ok(value)
}

#[cfg(test)]
mod tests;
