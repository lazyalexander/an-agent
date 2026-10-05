//! Names the plugin registered, and the config and env it injects.
//! Config is a TOML document (`WorkspaceConfig`). Env is markdown text.

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

/// The workspace's hard requirements, one TOML document per registered
/// config name: which spools (pinned), their mount parameters, and the
/// hook chains. Strict: unknown fields are refused — loosening is a
/// deliberate schema change, not a parse accident. IO contracts are not
/// here: they belong to the spool declaration, and matching is mutual
/// (2026-10-05 ruling — a workspace-side filter was tried and withdrawn).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceConfig {
    /// Spool packages the workspace asks for, pinned by version. Mounting
    /// (with the requires-closure check) is the host's follow-up, not this
    /// document's act.
    #[serde(default)]
    pub spool: Vec<SpoolRequirement>,
    /// Hook chain order and parameters. Implementations live in the host;
    /// the kernel validates shape only, never that a named hook exists.
    #[serde(default)]
    pub hooks: Hooks,
}

/// One pinned spool the workspace asks for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpoolRequirement {
    pub name: String,
    /// Exact `x.y.z`, same discipline as the spool shelf.
    pub version: String,
    /// Mount config handed to the spool body.
    #[serde(default)]
    pub config: toml::Table,
}

/// One hook handler: a tagged mechanism, never a bare name. Data declares
/// *how* to invoke, the same shape as codex's `HookHandlerConfig`
/// (command/mcp/prompt/agent) — minus `command`: arbitrary host commands
/// would bypass admission, and if one is ever wanted it is admitted like
/// bash, not smuggled in as a hook.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum HookHandler {
    /// A spool from the shelf, pinned. A rhai policy is the natural
    /// gate/transform: pure computation over the event, verdict out.
    Spool { name: String, version: String },
    /// An MCP tool call — the only kind of tool we have. External-world
    /// checks (and their cost) stay visible as tool calls.
    Mcp {
        server: String,
        tool: String,
        #[serde(default)]
        input: toml::Table,
    },
}

/// Before/after hook chains around spool delivery: two fixed points on
/// the delivery path, not open-ended names.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hooks {
    #[serde(default)]
    pub before: Vec<HookHandler>,
    #[serde(default)]
    pub after: Vec<HookHandler>,
}

impl WorkspaceConfig {
    /// Shape checks beyond parsing. Refuses what TOML cannot: empty or
    /// duplicated names, loose versions.
    fn validate(&self) -> Result<(), ControlError> {
        let mut spools = HashSet::new();
        for req in &self.spool {
            if req.name.is_empty() {
                return Err(ControlError::InvalidConfig("empty spool name".into()));
            }
            if !spools.insert(req.name.as_str()) {
                return Err(ControlError::InvalidConfig(format!(
                    "duplicate spool: {}",
                    req.name
                )));
            }
            if !exact_version(&req.version) {
                return Err(ControlError::InvalidConfig(format!(
                    "spool {} version must be exact x.y.z: {}",
                    req.name, req.version
                )));
            }
        }
        for hook in self.hooks.before.iter().chain(self.hooks.after.iter()) {
            match hook {
                HookHandler::Spool { name, version } => {
                    if name.is_empty() {
                        return Err(ControlError::InvalidConfig("empty hook spool name".into()));
                    }
                    if !exact_version(version) {
                        return Err(ControlError::InvalidConfig(format!(
                            "hook spool {name} version must be exact x.y.z: {version}"
                        )));
                    }
                }
                HookHandler::Mcp { server, tool, .. } => {
                    if server.is_empty() || tool.is_empty() {
                        return Err(ControlError::InvalidConfig(
                            "hook mcp server and tool must be non-empty".into(),
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

/// Exact `x.y.z` — the shelf pins versions, so anything naming a spool
/// does too.
fn exact_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
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
    /// The kernel admits by name only: what a spool accepts is the spool's
    /// declaration, and matching happens at bind time, not here.
    pub fn push_event(&self, name: &str, body: &str) -> Result<WorkspaceRecord, ControlError> {
        self.admit("event", name)?;
        Ok(self.inner.workspace.push_event(name, body)?)
    }

    /// Store one TOML document under a registered name. The host injects
    /// the bytes. The kernel parses them as a `WorkspaceConfig` and checks
    /// the shape. Identical bytes for that name keep the previous id.
    pub fn put_config(&self, name: &str, bytes: &[u8]) -> Result<WorkspaceRecord, ControlError> {
        self.admit("config", name)?;
        let text = std::str::from_utf8(bytes)
            .map_err(|err| ControlError::InvalidConfig(err.to_string()))?;
        let parsed: WorkspaceConfig =
            toml::from_str(text).map_err(|err| ControlError::InvalidConfig(err.to_string()))?;
        parsed.validate()?;
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
