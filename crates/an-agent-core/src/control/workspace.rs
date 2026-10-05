//! Names the plugin registered, and the config and env it injects.
//! Config is a JSON document. Env is markdown text.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::{AgentControl, ControlError};
use crate::workspace::{WorkspaceCite, WorkspaceRecord};

/// One event name and the spools that receive it, in delivery order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventRoute {
    pub name: String,
    pub spools: Vec<String>,
}

/// Names a plugin declared at load. Stored as one snapshot, not a live query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registration {
    pub events: Vec<EventRoute>,
    pub config: Vec<String>,
    pub env: Vec<String>,
}

impl AgentControl {
    /// Record the plugin's watch list. Identical bytes keep the previous id.
    /// A different list is a new snapshot. Older bytes stay readable.
    pub fn register(&self, registration: &Registration) -> Result<WorkspaceRecord, ControlError> {
        validate_registration(registration)?;
        let bytes = serde_json::to_vec(registration)
            .map_err(|err| ControlError::InvalidRegistration(err.to_string()))?;
        Ok(self.inner.workspace.put_register(&bytes)?)
    }

    /// Bytes of one stored registration snapshot.
    pub fn registration_bytes(&self, sha256_hex: &str) -> Result<Vec<u8>, ControlError> {
        Ok(self.inner.workspace.register_bytes(sha256_hex)?)
    }

    /// Append one registered software event. Repeated text is a new event.
    pub fn push_event(&self, name: &str, body: &str) -> Result<WorkspaceRecord, ControlError> {
        self.admit("event", name)?;
        Ok(self.inner.workspace.push_event(name, body)?)
    }

    /// Store one JSON document under a registered name. The host injects
    /// the bytes. The kernel checks that they parse and does not read the
    /// fields. Identical bytes for that name keep the previous id.
    pub fn put_config(&self, name: &str, bytes: &[u8]) -> Result<WorkspaceRecord, ControlError> {
        self.admit("config", name)?;
        serde_json::from_slice::<serde_json::Value>(bytes)
            .map_err(|err| ControlError::InvalidConfig(err.to_string()))?;
        Ok(self.inner.workspace.put_config(name, bytes)?)
    }

    /// Replace the workspace markdown under a registered name. The kernel
    /// stores the text and does not parse it. Identical text for that name
    /// keeps the previous id.
    pub fn put_env(&self, name: &str, markdown: &str) -> Result<WorkspaceRecord, ControlError> {
        self.admit("env", name)?;
        Ok(self.inner.workspace.put_env(name, markdown)?)
    }

    pub fn config_bytes(&self, sha256_hex: &str) -> Result<Vec<u8>, ControlError> {
        Ok(self.inner.workspace.config_bytes(sha256_hex)?)
    }

    pub fn env_bytes(&self, sha256_hex: &str) -> Result<Vec<u8>, ControlError> {
        Ok(self.inner.workspace.env_bytes(sha256_hex)?)
    }

    /// Current config id, env generation id, and env text.
    pub fn workspace_cite(&self) -> Result<WorkspaceCite, ControlError> {
        Ok(self.inner.workspace.cite()?)
    }

    /// The workspace log. This is the listen port. It does not poll.
    pub fn workspace_log(&self) -> Result<Vec<WorkspaceRecord>, ControlError> {
        Ok(self.inner.workspace.log()?)
    }
    fn current_registration(&self) -> Result<Registration, ControlError> {
        let cite = self.inner.workspace.cite()?;
        let Some(sha) = cite.register_sha256 else {
            return Err(ControlError::NotRegistered);
        };
        let bytes = self.inner.workspace.register_bytes(&sha)?;
        serde_json::from_slice(&bytes)
            .map_err(|err| ControlError::InvalidRegistration(err.to_string()))
    }

    pub(super) fn spools_for(&self, name: &str) -> Result<Vec<String>, ControlError> {
        let registration = self.current_registration()?;
        registration
            .events
            .into_iter()
            .find(|route| route.name == name)
            .map(|route| route.spools)
            .ok_or_else(|| ControlError::Unregistered(name.to_string()))
    }

    fn admit(&self, list: &str, name: &str) -> Result<(), ControlError> {
        let registration = self.current_registration()?;
        let known = match list {
            "event" => registration.events.iter().any(|route| route.name == name),
            "config" => registration.config.iter().any(|item| item == name),
            "env" => registration.env.iter().any(|item| item == name),
            _ => false,
        };
        if known {
            Ok(())
        } else {
            Err(ControlError::Unregistered(name.to_string()))
        }
    }
}

fn validate_registration(registration: &Registration) -> Result<(), ControlError> {
    unique_names(
        registration.events.iter().map(|route| route.name.as_str()),
        "event",
    )?;
    unique_names(registration.config.iter().map(String::as_str), "config")?;
    unique_names(registration.env.iter().map(String::as_str), "env")?;
    for route in &registration.events {
        unique_names(route.spools.iter().map(String::as_str), "spool")?;
    }
    Ok(())
}

fn unique_names<'a>(names: impl Iterator<Item = &'a str>, kind: &str) -> Result<(), ControlError> {
    let mut seen = HashSet::new();
    for name in names {
        if name.is_empty() {
            return Err(ControlError::InvalidRegistration(format!(
                "empty {kind} name"
            )));
        }
        if !seen.insert(name) {
            return Err(ControlError::InvalidRegistration(format!(
                "duplicate {kind} name: {name}"
            )));
        }
    }
    Ok(())
}
