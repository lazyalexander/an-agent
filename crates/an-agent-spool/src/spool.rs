//! Spool descriptor. A spool declares an internal capability: constructor,
//! effect ceiling, pinned requires, inverse. The document is a grant
//! envelope, not a self-description — what a spool actually forms at
//! runtime is observed on the tape, not read off the declaration.
//! (The document subsists in the registry; the mounted instance is its
//! hypostasis.)
//!
//! A published body is addressed by its sha256 and is never rewritten. A
//! later version is a new body. Workspace recovery reads the old body back
//! by hash or by name and version. Dependencies and the inverse point at
//! other spools already in this registry.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use thiserror::Error;

use an_agent_core::act::{FileFacet, MemoryFacet, ModelFacet};

use crate::descriptor::{self, Net, Proc, SpecError};

#[derive(Debug, Error)]
pub enum SpoolError {
    #[error(transparent)]
    Spec(#[from] SpecError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Invalid(String),
}

fn invalid(msg: impl Into<String>) -> SpoolError {
    SpoolError::Invalid(msg.into())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Flow {
    None,
    In,
    Out,
    Both,
}

/// What kind of mounted body this spool forms. `Beat` is a pure function
/// event → reply. `Policy` is an agent body: it yields steps and core
/// drives them (model calls admitted and taped per step). `Gate` is a
/// hook-chain body — built by the host's hook wiring, never mounted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sort {
    #[default]
    Beat,
    Gate,
    Policy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Faces {
    pub file: FileFacet,
    pub memory: MemoryFacet,
    pub net: Net,
    pub proc_: Proc,
    /// Model-call ceiling. Required and explicit, like the other faces:
    /// `none` refuses `invoke_model` at admission.
    pub model: ModelFacet,
    pub flow: Flow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inverse {
    /// Nothing to undo: unmount is detach-only (e.g. a pure policy whose
    /// effects all went through other spools' admitted acts).
    None,
    Irreversible,
    Spool {
        name: String,
        version: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Constructor {
    Rhai { script: String },
    Host { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Require {
    pub name: String,
    pub version: String,
}

/// One published spool. `body` is the YAML bytes that hash to `sha256`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpoolSpec {
    pub name: String,
    pub version: String,
    pub summary: String,
    /// Body sort: beat (default), gate, or policy. Absent means beat, so
    /// descriptors published before sorts parse unchanged.
    pub sort: Sort,
    pub constructor: Constructor,
    pub effect: Faces,
    pub requires: Vec<Require>,
    pub inverse: Inverse,
    pub config: Map<String, Value>,
    /// Workspace events this spool receives. Matched against the
    /// workspace register at mount (`AgentControl::mount_spool`) — the
    /// bind point — not on the delivery path.
    pub consumes: Vec<String>,
    /// Workspace events this spool may emit. Register membership is
    /// checked at mount; the emit channel itself is a later slice.
    pub produces: Vec<String>,
    pub sha256: String,
    pub body: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    v: u32,
    kind: String,
    name: String,
    version: String,
    summary: String,
    #[serde(default)]
    sort: Sort,
    constructor: String,
    script: Option<String>,
    host: Option<String>,
    effect: RawEffect,
    inverse: Value,
    #[serde(default)]
    config: Value,
    requires: Vec<RawRequire>,
    // Strict: both lists must be present (may be []) — the workspace-event
    // claim is explicit, never assumed.
    consumes: Vec<String>,
    produces: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRequire {
    name: String,
    version: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEffect {
    file: Value,
    memory: Value,
    net: Net,
    #[serde(rename = "proc")]
    proc_: Proc,
    // Strict: explicitly required, like every other face. An old body
    // without it fails to parse loudly rather than silently gaining
    // (or keeping) a model capability.
    model: Value,
    flow: Flow,
}

/// Parse a spool descriptor. `kind` must be `spool`. The constructor is
/// `rhai` or `host`. Permit is not a field. An MCP block is not a field.
pub fn parse(yaml: &str) -> Result<SpoolSpec, SpoolError> {
    if yaml.len() > descriptor::MAX_DESCRIPTOR_BYTES {
        return Err(invalid(format!(
            "descriptor too large: {} bytes (max {})",
            yaml.len(),
            descriptor::MAX_DESCRIPTOR_BYTES
        )));
    }
    let raw: Raw = serde_saphyr::from_str(yaml).map_err(SpecError::from)?;
    if raw.v != 1 {
        return Err(invalid(format!("v must be 1, got {}", raw.v)));
    }
    if raw.kind != "spool" {
        return Err(invalid(format!("kind must be spool, got {}", raw.kind)));
    }
    check_name(&raw.name)?;
    check_version(&raw.version)?;
    if raw.summary.trim().is_empty() {
        return Err(invalid("summary must be non-empty"));
    }
    let constructor = match raw.constructor.as_str() {
        "rhai" => {
            if raw.host.is_some() {
                return Err(invalid("constructor rhai must not carry a host name"));
            }
            let script = raw
                .script
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| invalid("constructor rhai requires a non-empty script"))?;
            Constructor::Rhai { script }
        }
        "host" => {
            if raw.script.is_some() {
                return Err(invalid("constructor host must not carry a script"));
            }
            let name = raw
                .host
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| invalid("constructor host requires a non-empty host name"))?;
            check_name(&name)?;
            Constructor::Host { name }
        }
        other => {
            return Err(invalid(format!(
                "spool constructor must be rhai or host, got {other}"
            )));
        }
    };
    let mut seen = std::collections::HashSet::new();
    let mut requires = Vec::with_capacity(raw.requires.len());
    for req in raw.requires {
        check_name(&req.name)?;
        check_version(&req.version)?;
        if req.name == raw.name {
            return Err(invalid(format!("{} requires itself", raw.name)));
        }
        if !seen.insert(req.name.clone()) {
            return Err(invalid(format!("duplicate require: {}", req.name)));
        }
        requires.push(Require {
            name: req.name,
            version: req.version,
        });
    }
    let file = descriptor::parse_file_facet_pub(raw.effect.file)?;
    let memory = descriptor::parse_memory_facet_pub(raw.effect.memory)?;
    let model = descriptor::parse_model_facet_pub(raw.effect.model)?;
    if matches!(memory, MemoryFacet::Forget { .. }) {
        return Err(invalid("effect.memory must be ignore or remember"));
    }
    if let FileFacet::Read { path, .. }
    | FileFacet::Write { path, .. }
    | FileFacet::ReadWrite { path, .. } = &file
    {
        descriptor::check_effect_path_pub(path)?;
    }
    let config = match raw.config {
        Value::Null => Map::new(),
        Value::Object(map) => map,
        _ => return Err(invalid("config must be a mapping")),
    };
    let consumes = check_event_names(&raw.consumes, "consumes")?;
    let produces = check_event_names(&raw.produces, "produces")?;
    let inverse = parse_inverse(raw.inverse)?;
    Ok(SpoolSpec {
        name: raw.name,
        version: raw.version,
        summary: raw.summary,
        sort: raw.sort,
        constructor,
        effect: Faces {
            file,
            memory,
            net: raw.effect.net,
            proc_: raw.effect.proc_,
            model,
            flow: raw.effect.flow,
        },
        requires,
        inverse,
        config,
        consumes,
        produces,
        sha256: sha256(yaml),
        body: yaml.to_string(),
    })
}

fn check_event_names(names: &[String], face: &str) -> Result<Vec<String>, SpoolError> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(names.len());
    for name in names {
        // Event names follow the register's discipline (non-empty), not
        // the spool-name one: dotted names like `discord.message` are
        // legitimate events. Whitespace is refused as a typo magnet.
        if name.is_empty() || name.chars().any(char::is_whitespace) {
            return Err(invalid(format!("invalid {face} event name: {name:?}")));
        }
        if !seen.insert(name.clone()) {
            return Err(invalid(format!("duplicate {face} event: {name}")));
        }
        out.push(name.clone());
    }
    Ok(out)
}

impl SpoolSpec {
    /// The mount-time claim, handed to `AgentControl::mount_spool` for the
    /// bind-time match against the workspace register.
    pub fn declaration(&self) -> an_agent_core::control::SpoolDeclaration {
        an_agent_core::control::SpoolDeclaration {
            consumes: self.consumes.clone(),
            produces: self.produces.clone(),
            model: self.effect.model,
        }
    }
}

fn parse_inverse(value: Value) -> Result<Inverse, SpoolError> {
    match value {
        Value::String(s) if s == "none" => Ok(Inverse::None),
        Value::String(s) if s == "irreversible" => Ok(Inverse::Irreversible),
        Value::Object(map) => {
            let name = map
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("inverse.name must be a string"))?
                .to_string();
            let version = map
                .get("version")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("inverse.version must be a string"))?
                .to_string();
            if map.len() != 2 {
                return Err(invalid("inverse mapping accepts only name and version"));
            }
            check_name(&name)?;
            check_version(&version)?;
            Ok(Inverse::Spool { name, version })
        }
        _ => Err(invalid(
            "inverse must be none, irreversible, or a name and version",
        )),
    }
}

mod closure;
mod cover;
mod registry;

pub use closure::Closure;
pub use cover::covers;
pub use registry::Registry;

fn sha256(yaml: &str) -> String {
    format!("{:x}", Sha256::digest(yaml.as_bytes()))
}

fn check_name(name: &str) -> Result<(), SpoolError> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
        && name.chars().next().is_some_and(|c| c.is_ascii_lowercase());
    if !ok {
        return Err(invalid(format!("invalid spool name: {name}")));
    }
    Ok(())
}

fn check_version(version: &str) -> Result<(), SpoolError> {
    let parts: Vec<&str> = version.split('.').collect();
    let ok = parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
    if !ok {
        return Err(invalid(format!("invalid version (want x.y.z): {version}")));
    }
    Ok(())
}

#[cfg(test)]
#[path = "spool/tests.rs"]
mod tests;
