//! Tool descriptor (YAML) admission: parse + strict validation.
//! Contract: tool-registry.md appendix v1.
//! Policy: strict first — reject anything ambiguous. Loosening candidates
//! are tracked in tool-registry.md, not in code comments.
//! Single-file validation covers the descriptor itself; presence checks
//! (every `requires` entry exists) live in `check_requires_present`,
//! fed by the registry's index set.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use an_agent_core::act::{FileFacet, MemoryFacet};

#[derive(Debug, Error)]
pub enum SpecError {
    #[error("yaml: {0}")]
    Yaml(#[from] serde_saphyr::Error),
    #[error("{0}")]
    Invalid(String),
}

fn invalid(msg: impl Into<String>) -> SpecError {
    SpecError::Invalid(msg.into())
}

// --- raw serde shapes ---
// Strict: every struct denies unknown fields — typos must fail loudly.

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDescriptor {
    v: u32,
    name: String,
    version: String,
    summary: String,
    constructor: String,
    script: Option<String>,
    mcp: Option<RawMcp>,
    effect: RawEffect,
    // Strict: the field must be present (may be []).
    requires: Option<Vec<RawRequire>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMcp {
    transport: String,
    command: Vec<String>,
    tool: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRequire {
    name: String,
    version: String,
}

// Strict: all four effect faces explicitly required.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEffect {
    // file/memory arrive as raw Values, key-checked in validate_effect:
    // internally tagged enums silently swallow extra keys (admission bypass).
    file: serde_json::Value,
    memory: serde_json::Value,
    net: Net,
    #[serde(rename = "proc")]
    proc_: Proc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Net {
    None,
    Egress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Proc {
    None,
    Spawn,
}

// --- validated public shape ---

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Descriptor {
    pub name: String,
    pub version: String,
    pub summary: String,
    pub constructor: Constructor,
    pub effect: ToolEffect,
    pub requires: Vec<Require>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Constructor {
    Rhai { script: String },
    // Transport is strict stdio-only and not stored.
    Mcp { command: Vec<String>, tool: String },
    // No Wasm/Builtin variants: builtin stays kernel-reserved and never
    // enters the registry; wasm awaits its sandbox slice.
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolEffect {
    pub file: FileFacet,
    pub memory: MemoryFacet,
    pub net: Net,
    pub proc_: Proc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Require {
    pub name: String,
    pub version: String,
}

/// sha256 hex of the raw YAML text — the registry's content address.
pub fn content_hash(yaml: &str) -> String {
    let mut h = Sha256::new();
    h.update(yaml.as_bytes());
    format!("{:x}", h.finalize())
}

/// Hard input cap, enforced before the parser ever sees the bytes:
/// pathological or oversized documents are rejected as a DoS surface
/// regardless of parser internals. 64 KiB is far above any legitimate v1
/// descriptor (~2 KiB observed, inline script included).
pub(crate) const MAX_DESCRIPTOR_BYTES: usize = 64 * 1024;

pub fn parse(yaml: &str) -> Result<Descriptor, SpecError> {
    if yaml.len() > MAX_DESCRIPTOR_BYTES {
        return Err(invalid(format!(
            "descriptor too large: {} bytes (max {MAX_DESCRIPTOR_BYTES})",
            yaml.len()
        )));
    }
    let raw: RawDescriptor = serde_saphyr::from_str(yaml)?;
    validate(raw)
}

fn validate(raw: RawDescriptor) -> Result<Descriptor, SpecError> {
    // Strict: v1 only.
    if raw.v != 1 {
        return Err(invalid(format!("v must be 1, got {}", raw.v)));
    }
    check_name(&raw.name)?;
    check_version(&raw.version)?;
    if raw.summary.trim().is_empty() {
        return Err(invalid("summary must be non-empty"));
    }

    let constructor = match raw.constructor.as_str() {
        "rhai" => {
            let script = raw
                .script
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| invalid("constructor rhai requires a non-empty script"))?;
            if raw.mcp.is_some() {
                return Err(invalid("constructor rhai must not carry an mcp block"));
            }
            Constructor::Rhai { script }
        }
        "mcp" => {
            if raw.script.is_some() {
                return Err(invalid("constructor mcp must not carry a script"));
            }
            let mcp = raw
                .mcp
                .ok_or_else(|| invalid("constructor mcp requires an mcp block"))?;
            // Strict: stdio only.
            if mcp.transport != "stdio" {
                return Err(invalid(format!(
                    "mcp transport must be stdio, got {}",
                    mcp.transport
                )));
            }
            if mcp.command.is_empty() || mcp.command.iter().any(|c| c.trim().is_empty()) {
                return Err(invalid("mcp command must be non-empty"));
            }
            if mcp.tool.trim().is_empty() {
                return Err(invalid("mcp tool must be non-empty"));
            }
            Constructor::Mcp {
                command: mcp.command,
                tool: mcp.tool,
            }
        }
        other => {
            return Err(invalid(format!(
                "unknown constructor {other}; supported: rhai, mcp"
            )));
        }
    };

    let effect = validate_effect(raw.effect)?;

    let requires = raw
        .requires
        .ok_or_else(|| invalid("requires is required; use [] for no deps"))?;
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(requires.len());
    for r in requires {
        check_name(&r.name)?;
        check_version(&r.version)?;
        // Strict: a duplicate name or self-reference inside the flattened
        // closure is a flattener bug — always reject.
        if r.name == raw.name {
            return Err(invalid(format!("{} requires itself", raw.name)));
        }
        if !seen.insert(r.name.clone()) {
            return Err(invalid(format!("duplicate require: {}", r.name)));
        }
        out.push(Require {
            name: r.name,
            version: r.version,
        });
    }

    Ok(Descriptor {
        name: raw.name,
        version: raw.version,
        summary: raw.summary,
        constructor,
        effect,
        requires: out,
    })
}

fn validate_effect(raw: RawEffect) -> Result<ToolEffect, SpecError> {
    let file = parse_file_facet(raw.file)?;
    let memory = parse_memory_facet(raw.memory)?;
    match &file {
        FileFacet::Read { path, .. } | FileFacet::Write { path, .. } => {
            check_effect_path(path)?;
        }
        FileFacet::ReadWrite { path, .. } => check_effect_path(path)?,
        FileFacet::None | FileFacet::Unbounded => {}
    }
    // Strict: effect describes ignore/remember only; forget is a runtime
    // operation, not a capability declaration.
    if let MemoryFacet::Forget { .. } = memory {
        return Err(invalid("effect.memory must be ignore or remember"));
    }
    Ok(ToolEffect {
        file,
        memory,
        net: raw.net,
        proc_: raw.proc_,
    })
}

// --- facet hand-validation ---
// serde-json Values stand in for a YAML AST: keys are always strings,
// which is exactly what the key-by-key checks below want.
type RawMap = serde_json::Map<String, serde_json::Value>;

fn facet_op(value: serde_json::Value, face: &str) -> Result<(String, RawMap), SpecError> {
    let mut map = match value {
        serde_json::Value::Object(m) => m,
        // No scalar shorthand (file: none): one canonical mapping form,
        // one syntax to validate.
        _ => {
            return Err(invalid(format!(
                "effect.{face} must be a mapping with an op key"
            )));
        }
    };
    let op = map
        .remove("op")
        .and_then(|v| v.as_str().map(str::to_string))
        .ok_or_else(|| invalid(format!("effect.{face} requires a string op key")))?;
    Ok((op, map))
}

fn reject_extra_keys(
    face: &str,
    op: &str,
    map: &RawMap,
    allowed: &[&str],
) -> Result<(), SpecError> {
    for k in map.keys() {
        if !allowed.contains(&k.as_str()) {
            return Err(invalid(format!(
                "unknown key in effect.{face} (op {op}): {k}"
            )));
        }
    }
    Ok(())
}

fn optional_string(face: &str, map: &RawMap, key: &str) -> Result<Option<String>, SpecError> {
    match map.get(key) {
        None => Ok(None),
        Some(v) => v
            .as_str()
            .map(|s| Some(s.to_string()))
            .ok_or_else(|| invalid(format!("effect.{face}.{key} must be a string"))),
    }
}

fn required_string(face: &str, map: &RawMap, key: &str) -> Result<String, SpecError> {
    optional_string(face, map, key)?
        .ok_or_else(|| invalid(format!("effect.{face} requires a string {key}")))
}

pub(crate) fn parse_file_facet_pub(value: serde_json::Value) -> Result<FileFacet, SpecError> {
    parse_file_facet(value)
}

fn parse_file_facet(value: serde_json::Value) -> Result<FileFacet, SpecError> {
    let (op, map) = facet_op(value, "file")?;
    let facet = match op.as_str() {
        "none" => {
            reject_extra_keys("file", &op, &map, &[])?;
            FileFacet::None
        }
        "unbounded" => {
            reject_extra_keys("file", &op, &map, &[])?;
            FileFacet::Unbounded
        }
        "r" | "w" | "rw" => {
            reject_extra_keys("file", &op, &map, &["path", "recursive"])?;
            let path = required_string("file", &map, "path")?;
            let recursive = match map.get("recursive") {
                None => false,
                Some(v) => v
                    .as_bool()
                    .ok_or_else(|| invalid("effect.file.recursive must be a bool"))?,
            };
            match op.as_str() {
                "r" => FileFacet::Read { path, recursive },
                "w" => FileFacet::Write { path, recursive },
                _ => FileFacet::ReadWrite { path, recursive },
            }
        }
        other => return Err(invalid(format!("unknown effect.file op: {other}"))),
    };
    Ok(facet)
}

pub(crate) fn parse_memory_facet_pub(value: serde_json::Value) -> Result<MemoryFacet, SpecError> {
    parse_memory_facet(value)
}

fn parse_memory_facet(value: serde_json::Value) -> Result<MemoryFacet, SpecError> {
    let (op, map) = facet_op(value, "memory")?;
    let facet = match op.as_str() {
        "ignore" => {
            reject_extra_keys("memory", &op, &map, &[])?;
            MemoryFacet::Ignore
        }
        "remember" => {
            reject_extra_keys("memory", &op, &map, &["aspect"])?;
            MemoryFacet::Remember {
                aspect: optional_string("memory", &map, "aspect")?,
            }
        }
        "forget" => {
            reject_extra_keys("memory", &op, &map, &["rememberId"])?;
            MemoryFacet::Forget {
                remember_id: required_string("memory", &map, "rememberId")?,
            }
        }
        other => return Err(invalid(format!("unknown effect.memory op: {other}"))),
    };
    Ok(facet)
}

/// Effect file paths must be clean absolute paths and may not point at
/// relative locations inside a workspace or a session.
pub(crate) fn check_effect_path_pub(path: &str) -> Result<(), SpecError> {
    check_effect_path(path)
}

fn check_effect_path(path: &str) -> Result<(), SpecError> {
    let p = std::path::Path::new(path);
    if !p.is_absolute() {
        return Err(invalid(format!(
            "effect file path must be absolute: {path}"
        )));
    }
    if path.contains('~')
        || p.components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(invalid(format!("effect file path must be clean: {path}")));
    }
    Ok(())
}

// Strict: lowercase snake/kebab.
fn check_name(name: &str) -> Result<(), SpecError> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
        && name.chars().next().is_some_and(|c| c.is_ascii_lowercase());
    if !ok {
        return Err(invalid(format!("invalid tool name: {name}")));
    }
    Ok(())
}

// Strict: exact x.y.z.
fn check_version(version: &str) -> Result<(), SpecError> {
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

/// Verify each `requires` entry exists in the registry index —
/// lookup only, no closure computation, no graph walk.
pub fn check_requires_present(
    d: &Descriptor,
    index: &HashSet<(String, String)>,
) -> Result<(), SpecError> {
    for r in &d.requires {
        if !index.contains(&(r.name.clone(), r.version.clone())) {
            return Err(invalid(format!(
                "required tool not in registry: {} {}",
                r.name, r.version
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "descriptor/tests.rs"]
mod tests;
