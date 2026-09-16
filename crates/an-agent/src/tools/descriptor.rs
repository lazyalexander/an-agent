//! Tool descriptor (YAML) admission: parse + strict validation.
//! Contract: tool-registry.md appendix v1 (T1/T4/T5/T6).
//! Policy: strict first — reject anything ambiguous; each strictness rule
//! carries a comment marking its possible future loosening.
//! Single-file validation covers the descriptor itself; presence checks
//! (every `requires` entry exists) live in `check_requires_present`,
//! fed by the registry's index set.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::act::{FileFacet, MemoryFacet};

#[derive(Debug, Error)]
pub enum SpecError {
    #[error("yaml: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("{0}")]
    Invalid(String),
}

fn invalid(msg: impl Into<String>) -> SpecError {
    SpecError::Invalid(msg.into())
}

// --- raw serde shapes ---
// Strict: every struct denies unknown fields — typos must fail loudly.
// Future loosening: when descriptor versions must coexist, downgrade
// unknown fields to a recorded warning.
// Known hole (serde limitation): internally tagged enums (FileFacet/
// MemoryFacet) silently swallow extra keys, e.g. {op: none, path: x}.
// Future tightening: hand-rolled Value-level validation.

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
    // Future loosening: missing means [].
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
// Future loosening: file/memory default to none/ignore.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEffect {
    file: FileFacet,
    memory: MemoryFacet,
    net: Net,
    #[serde(rename = "proc")]
    proc_: Proc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Net {
    None,
    Egress,
    // Future loosening: listen; domain-allowlisted egress (to: [...]).
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Proc {
    None,
    Spawn,
    // Future loosening: argv-shape restriction on spawn (e.g. only: [...]).
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
    // Transport is strict stdio-only and not stored; future loosening:
    // http/sse, upgrading this to an enum field.
    Mcp { command: Vec<String>, tool: String },
    // Future loosening: Wasm { .. }; Builtin stays kernel-reserved and
    // never enters the registry.
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

/// sha256 hex of the raw YAML text — the registry's content address (T1).
pub fn content_hash(yaml: &str) -> String {
    let mut h = Sha256::new();
    h.update(yaml.as_bytes());
    format!("{:x}", h.finalize())
}

pub fn parse(yaml: &str) -> Result<Descriptor, SpecError> {
    let raw: RawDescriptor = serde_yaml::from_str(yaml)?;
    validate(raw)
}

fn validate(raw: RawDescriptor) -> Result<Descriptor, SpecError> {
    // Strict: v1 only. Future loosening: per-version dispatch from v2 on
    // (E1); old versions stay readable forever.
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
            // Strict: stdio only. Future loosening: http/sse transports.
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
    match &raw.file {
        FileFacet::Read { path, .. } | FileFacet::Write { path, .. } => {
            check_effect_path(path)?;
        }
        FileFacet::ReadWrite { path, .. } => check_effect_path(path)?,
        FileFacet::None | FileFacet::Unbounded => {}
    }
    // Strict: effect describes ignore/remember only; forget is a runtime
    // operation, not a capability declaration.
    // Future loosening: admitting forget to the effect vocabulary requires
    // designing its audit semantics first.
    if let MemoryFacet::Forget { .. } = raw.memory {
        return Err(invalid("effect.memory must be ignore or remember"));
    }
    Ok(ToolEffect {
        file: raw.file,
        memory: raw.memory,
        net: raw.net,
        proc_: raw.proc_,
    })
}

/// T6 made mechanical: effect file paths must be clean absolute paths and
/// may not point at relative locations inside a workplace/session.
/// Future loosening: anchored-relative paths (relative to a declared root,
/// resolved by the registry).
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

// Strict: lowercase snake/kebab. Future loosening: namespaces (org/name),
// uppercase.
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

// Strict: exact x.y.z. Future loosening: prerelease/build metadata;
// `requires` stays exact-pinned regardless.
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

/// T4: verify each `requires` entry exists in the registry index —
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
mod tests {
    use super::*;

    const BASH: &str = r#"
v: 1
name: bash
version: 1.0.0
constructor: rhai
script: |
  let out = exec(params.command, #{ timeout: 120 });
  out.stdout
summary: Run a shell command
effect:
  proc: spawn
  file: { op: unbounded }
  net: egress
  memory: { op: ignore }
requires: []
"#;

    const MCP: &str = r#"
v: 1
name: fs_read
version: 1.0.0
constructor: mcp
mcp:
  transport: stdio
  command: ["npx", "-y", "server-filesystem", "/srv"]
  tool: read_file
summary: Read files under /srv
effect:
  file: { op: r, path: "/srv" }
  net: none
  proc: none
  memory: { op: ignore }
requires: []
"#;

    #[test]
    fn parses_rhai_and_mcp() {
        let d = parse(BASH).unwrap();
        assert!(matches!(d.constructor, Constructor::Rhai { .. }));
        assert_eq!(d.effect.net, Net::Egress);
        assert_eq!(d.effect.proc_, Proc::Spawn);
        let m = parse(MCP).unwrap();
        assert!(matches!(m.constructor, Constructor::Mcp { .. }));
    }

    #[test]
    fn rejects_unknown_field() {
        let bad = BASH.replace("summary:", "summray:");
        assert!(parse(&bad).is_err());
    }

    #[test]
    fn rejects_wrong_v_and_sloppy_identity() {
        assert!(parse(&BASH.replace("v: 1", "v: 2")).is_err());
        assert!(parse(&BASH.replace("name: bash", "name: Bash")).is_err());
        assert!(parse(&BASH.replace("version: 1.0.0", "version: 1.0")).is_err());
    }

    #[test]
    fn enforces_constructor_block_pairing() {
        let no_script = BASH.replace(
            "script: |\n  let out = exec(params.command, #{ timeout: 120 });\n  out.stdout\n",
            "",
        );
        assert!(parse(&no_script).is_err());
        let both = BASH.replace(
            "requires: []",
            "mcp: { transport: stdio, command: [\"x\"], tool: t }\nrequires: []",
        );
        assert!(parse(&both).is_err());
        assert!(parse(&MCP.replace("transport: stdio", "transport: http")).is_err());
    }

    #[test]
    fn effect_faces_required_and_constrained() {
        let no_net = BASH.replace("  net: egress\n", "");
        assert!(parse(&no_net).is_err());
        let rel = MCP.replace("path: \"/srv\"", "path: \"srv/data\"");
        assert!(parse(&rel).is_err());
        let forget = BASH.replace(
            "memory: { op: ignore }",
            "memory: { op: forget, rememberId: x }",
        );
        assert!(parse(&forget).is_err());
    }

    #[test]
    fn requires_strictness() {
        let missing = BASH.replace("requires: []\n", "");
        assert!(parse(&missing).is_err());
        let self_ref = BASH.replace("requires: []", "requires: [{ name: bash, version: 1.0.0 }]");
        assert!(parse(&self_ref).is_err());
        let dup = BASH.replace(
            "requires: []",
            "requires:\n  - { name: a, version: 1.0.0 }\n  - { name: a, version: 2.0.0 }",
        );
        assert!(parse(&dup).is_err());
    }

    #[test]
    fn presence_check_is_lookup_only() {
        let d = parse(&BASH.replace(
            "requires: []",
            "requires: [{ name: http_fetch, version: 1.4.0 }]",
        ))
        .unwrap();
        let mut index = HashSet::new();
        assert!(check_requires_present(&d, &index).is_err());
        index.insert(("http_fetch".into(), "1.4.0".into()));
        assert!(check_requires_present(&d, &index).is_ok());
    }

    #[test]
    fn content_hash_is_stable() {
        assert_eq!(content_hash(BASH), content_hash(BASH));
        assert_ne!(content_hash(BASH), content_hash(MCP));
    }
}
