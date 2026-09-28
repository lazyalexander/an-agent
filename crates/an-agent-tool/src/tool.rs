//! Plugin descriptor. A published body is addressed by its sha256 and is
//! never rewritten. A later version is a new body. Workspace recovery
//! reads the old body back by hash or by name and version. Dependencies
//! and the inverse point at other plugins already in this registry.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use thiserror::Error;

use an_agent_core::act::{FileFacet, MemoryFacet};

use crate::descriptor::{self, Net, Proc, SpecError};

#[derive(Debug, Error)]
pub enum ToolError {
    #[error(transparent)]
    Spec(#[from] SpecError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Invalid(String),
}

fn invalid(msg: impl Into<String>) -> ToolError {
    ToolError::Invalid(msg.into())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Flow {
    None,
    In,
    Out,
    Both,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Faces {
    pub file: FileFacet,
    pub memory: MemoryFacet,
    pub net: Net,
    pub proc_: Proc,
    pub flow: Flow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inverse {
    Irreversible,
    Plugin { name: String, version: String },
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

/// One published plugin. `body` is the YAML bytes that hash to `sha256`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginSpec {
    pub name: String,
    pub version: String,
    pub summary: String,
    pub constructor: Constructor,
    pub effect: Faces,
    pub requires: Vec<Require>,
    pub inverse: Inverse,
    pub config: Map<String, Value>,
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
    constructor: String,
    script: Option<String>,
    host: Option<String>,
    effect: RawEffect,
    inverse: Value,
    #[serde(default)]
    config: Value,
    requires: Vec<RawRequire>,
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
    flow: Flow,
}

/// Parse a plugin descriptor. `kind` must be `plugin`. The constructor is
/// `rhai` or `host`. Permit is not a field. An MCP block is not a field.
pub fn parse(yaml: &str) -> Result<PluginSpec, ToolError> {
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
    if raw.kind != "plugin" {
        return Err(invalid(format!("kind must be plugin, got {}", raw.kind)));
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
                "plugin constructor must be rhai or host, got {other}"
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
    let inverse = parse_inverse(raw.inverse)?;
    Ok(PluginSpec {
        name: raw.name,
        version: raw.version,
        summary: raw.summary,
        constructor,
        effect: Faces {
            file,
            memory,
            net: raw.effect.net,
            proc_: raw.effect.proc_,
            flow: raw.effect.flow,
        },
        requires,
        inverse,
        config,
        sha256: sha256(yaml),
        body: yaml.to_string(),
    })
}

fn parse_inverse(value: Value) -> Result<Inverse, ToolError> {
    match value {
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
            Ok(Inverse::Plugin { name, version })
        }
        _ => Err(invalid(
            "inverse must be irreversible or a name and version",
        )),
    }
}

/// `ceiling` covers `effect` when every face of the effect is inside it.
/// Rights and the card's sentence both use this. The caller decides whether
/// a miss is Forbidden or Deny.
pub fn covers(ceiling: &Faces, effect: &Faces) -> bool {
    file_covers(&ceiling.file, &effect.file)
        && memory_covers(&ceiling.memory, &effect.memory)
        && net_covers(ceiling.net, effect.net)
        && proc_covers(ceiling.proc_, effect.proc_)
        && flow_covers(ceiling.flow, effect.flow)
}

fn file_covers(ceiling: &FileFacet, effect: &FileFacet) -> bool {
    if matches!(effect, FileFacet::None) {
        return true;
    }
    match (ceiling, effect) {
        (FileFacet::Unbounded, FileFacet::Unbounded) => true,
        (
            FileFacet::Unbounded,
            FileFacet::Read { .. } | FileFacet::Write { .. } | FileFacet::ReadWrite { .. },
        ) => true,
        (
            FileFacet::Read {
                path: root,
                recursive,
            },
            FileFacet::Read { path, .. },
        )
        | (
            FileFacet::Write {
                path: root,
                recursive,
            },
            FileFacet::Write { path, .. },
        )
        | (
            FileFacet::ReadWrite {
                path: root,
                recursive,
            },
            FileFacet::Read { path, .. }
            | FileFacet::Write { path, .. }
            | FileFacet::ReadWrite { path, .. },
        ) => path_covers(root, *recursive, path),
        _ => false,
    }
}

fn path_covers(root: &str, recursive: bool, path: &str) -> bool {
    if path == root {
        return true;
    }
    recursive && path.starts_with(&format!("{root}/"))
}

fn memory_covers(ceiling: &MemoryFacet, effect: &MemoryFacet) -> bool {
    match (ceiling, effect) {
        (_, MemoryFacet::Ignore) => true,
        (MemoryFacet::Remember { aspect: None }, MemoryFacet::Remember { .. }) => true,
        (MemoryFacet::Remember { aspect: Some(a) }, MemoryFacet::Remember { aspect: Some(b) }) => {
            a == b
        }
        _ => false,
    }
}

fn net_covers(ceiling: Net, effect: Net) -> bool {
    matches!(
        (ceiling, effect),
        (Net::None, Net::None) | (Net::Egress, Net::None | Net::Egress)
    )
}

fn proc_covers(ceiling: Proc, effect: Proc) -> bool {
    matches!(
        (ceiling, effect),
        (Proc::None, Proc::None) | (Proc::Spawn, Proc::None | Proc::Spawn)
    )
}

fn flow_covers(ceiling: Flow, effect: Flow) -> bool {
    matches!(
        (ceiling, effect),
        (Flow::None, Flow::None)
            | (Flow::In, Flow::None | Flow::In)
            | (Flow::Out, Flow::None | Flow::Out)
            | (Flow::Both, _)
    )
}

/// Append-only registry of plugin bodies. `blobs/<sha256>` is the YAML.
/// `index.jsonl` records name, version, and hash. Neither file is rewritten.
pub struct Registry {
    root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IndexLine {
    name: String,
    version: String,
    sha256: String,
    inverse: IndexInverse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum IndexInverse {
    Irreversible(String),
    Plugin { name: String, version: String },
}

impl Registry {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, ToolError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(root.join("blobs"))?;
        let index = root.join("index.jsonl");
        if !index.exists() {
            OpenOptions::new().create(true).append(true).open(&index)?;
        }
        Ok(Self { root })
    }

    /// Store `yaml` if this name and version are new. The same bytes may
    /// be published again. A different body for the same version is refused,
    /// so an old workspace write can still name the descriptor that made it.
    pub fn publish(&self, yaml: &str) -> Result<PluginSpec, ToolError> {
        let spec = parse(yaml)?;
        for req in &spec.requires {
            if self.lookup(&req.name, &req.version)?.is_none() {
                return Err(invalid(format!(
                    "required plugin is not published: {} {}",
                    req.name, req.version
                )));
            }
        }
        if let Inverse::Plugin { name, version } = &spec.inverse
            && self.lookup(name, version)?.is_none()
        {
            return Err(invalid(format!(
                "inverse plugin is not published: {name} {version}"
            )));
        }
        if let Some(existing) = self.lookup(&spec.name, &spec.version)? {
            if existing.sha256 != spec.sha256 {
                return Err(invalid(format!(
                    "version {} {} is already published as {}",
                    spec.name, spec.version, existing.sha256
                )));
            }
            return Ok(existing);
        }
        let blob = self.blob_path(&spec.sha256);
        if !blob.exists() {
            let mut tmp = blob.clone();
            tmp.set_extension("tmp");
            fs::write(&tmp, yaml.as_bytes())?;
            fs::rename(&tmp, &blob)?;
        }
        let line = IndexLine {
            name: spec.name.clone(),
            version: spec.version.clone(),
            sha256: spec.sha256.clone(),
            inverse: match &spec.inverse {
                Inverse::Irreversible => IndexInverse::Irreversible("irreversible".into()),
                Inverse::Plugin { name, version } => IndexInverse::Plugin {
                    name: name.clone(),
                    version: version.clone(),
                },
            },
        };
        let mut index = OpenOptions::new().append(true).open(self.index_path())?;
        writeln!(index, "{}", serde_json::to_string(&line)?)?;
        Ok(spec)
    }

    pub fn recover(&self, name: &str, version: &str) -> Result<PluginSpec, ToolError> {
        self.lookup(name, version)?
            .ok_or_else(|| invalid(format!("plugin not published: {name} {version}")))
    }

    pub fn recover_hash(&self, sha256: &str) -> Result<PluginSpec, ToolError> {
        let body = fs::read_to_string(self.blob_path(sha256))?;
        let spec = parse(&body)?;
        if spec.sha256 != sha256 {
            return Err(invalid(format!("blob {sha256} does not hash to its name")));
        }
        Ok(spec)
    }

    /// Every published version of `name`, oldest first.
    pub fn versions(&self, name: &str) -> Result<Vec<PluginSpec>, ToolError> {
        let mut out = Vec::new();
        for line in self.lines()? {
            if line.name == name {
                out.push(self.recover_hash(&line.sha256)?);
            }
        }
        Ok(out)
    }

    fn lookup(&self, name: &str, version: &str) -> Result<Option<PluginSpec>, ToolError> {
        for line in self.lines()? {
            if line.name == name && line.version == version {
                return Ok(Some(self.recover_hash(&line.sha256)?));
            }
        }
        Ok(None)
    }

    fn lines(&self) -> Result<Vec<IndexLine>, ToolError> {
        let text = fs::read_to_string(self.index_path())?;
        let mut out = Vec::new();
        for (i, line) in text.lines().enumerate() {
            if line.is_empty() {
                continue;
            }
            out.push(
                serde_json::from_str(line)
                    .map_err(|e| invalid(format!("index line {}: {e}", i + 1)))?,
            );
        }
        Ok(out)
    }

    fn index_path(&self) -> PathBuf {
        self.root.join("index.jsonl")
    }

    fn blob_path(&self, sha256: &str) -> PathBuf {
        self.root.join("blobs").join(sha256)
    }
}

fn sha256(yaml: &str) -> String {
    format!("{:x}", Sha256::digest(yaml.as_bytes()))
}

fn check_name(name: &str) -> Result<(), ToolError> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
        && name.chars().next().is_some_and(|c| c.is_ascii_lowercase());
    if !ok {
        return Err(invalid(format!("invalid plugin name: {name}")));
    }
    Ok(())
}

fn check_version(version: &str) -> Result<(), ToolError> {
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
mod tests {
    use super::*;
    use an_agent_core::testkit::TempDir;

    fn read_yaml() -> String {
        r#"
v: 1
kind: plugin
name: fs_read
version: 1.0.0
summary: Read a workspace file
constructor: host
host: fs_read
effect:
  file: { op: r, path: "/srv" }
  memory: { op: ignore }
  net: none
  proc: none
  flow: in
inverse: irreversible
requires: []
"#
        .to_string()
    }

    fn write_yaml() -> String {
        r#"
v: 1
kind: plugin
name: fs_write
version: 1.0.0
summary: Write a workspace file
constructor: rhai
script: |
  write(params.path, params.body)
effect:
  file: { op: w, path: "/srv", recursive: true }
  memory: { op: ignore }
  net: none
  proc: none
  flow: none
inverse:
  name: fs_restore
  version: 1.0.0
config:
  timeout_ms: 1000
requires:
  - { name: fs_read, version: 1.0.0 }
"#
        .to_string()
    }

    #[test]
    fn parses_a_hosted_plugin() {
        let spec = parse(&read_yaml()).unwrap();
        assert!(matches!(spec.constructor, Constructor::Host { .. }));
        assert_eq!(spec.effect.flow, Flow::In);
        assert_eq!(spec.inverse, Inverse::Irreversible);
        assert!(spec.requires.is_empty());
        assert_eq!(spec.sha256.len(), 64);
    }

    #[test]
    fn rejects_a_tool_an_mcp_block_and_a_permit() {
        let tool = read_yaml().replace("kind: plugin", "kind: tool");
        assert!(
            parse(&tool)
                .unwrap_err()
                .to_string()
                .contains("kind must be plugin")
        );
        let mcp = read_yaml().replace("constructor: host\nhost: fs_read", "constructor: mcp");
        assert!(
            parse(&mcp)
                .unwrap_err()
                .to_string()
                .contains("rhai or host")
        );
        let permit = read_yaml().replace("kind: plugin", "kind: plugin\npermit: go");
        assert!(
            parse(&permit)
                .unwrap_err()
                .to_string()
                .contains("unknown field")
        );
    }

    #[test]
    fn ceiling_is_per_face() {
        let read = parse(&read_yaml()).unwrap();
        let mut rights = read.effect.clone();
        assert!(covers(&rights, &read.effect));
        rights.net = Net::None;
        rights.flow = Flow::None;
        assert!(!covers(&rights, &read.effect));
        rights.flow = Flow::Both;
        rights.net = Net::Egress;
        assert!(covers(&rights, &read.effect));
    }

    #[test]
    fn published_versions_stay_readable() {
        let tmp = TempDir::new("plugins");
        let reg = Registry::open(tmp.path()).unwrap();
        let reader = reg.publish(&read_yaml()).unwrap();
        assert!(matches!(reader.constructor, Constructor::Host { .. }));
        let restore = read_yaml()
            .replace("name: fs_read", "name: fs_restore")
            .replace("host: fs_read", "host: fs_restore")
            .replace("summary: Read a workspace file", "summary: Restore");
        reg.publish(&restore).unwrap();
        let first = reg.publish(&write_yaml()).unwrap();
        let again = reg.publish(&write_yaml()).unwrap();
        assert_eq!(first.sha256, again.sha256);
        let changed = write_yaml().replace("timeout_ms: 1000", "timeout_ms: 2000");
        assert!(reg.publish(&changed).is_err());
        let next = write_yaml().replace(
            "name: fs_write\nversion: 1.0.0",
            "name: fs_write\nversion: 1.0.1",
        );
        reg.publish(&next).unwrap();
        let old = reg.recover("fs_write", "1.0.0").unwrap();
        assert_eq!(old.body, write_yaml());
        assert_eq!(old.sha256, first.sha256);
        let by_hash = reg.recover_hash(&first.sha256).unwrap();
        assert_eq!(by_hash.summary, "Write a workspace file");
        let history = reg.versions("fs_write").unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].version, "1.0.0");
        assert_eq!(history[1].version, "1.0.1");
        assert!(
            reg.publish(
                &write_yaml()
                    .replace(
                        "inverse:\n  name: fs_restore\n  version: 1.0.0",
                        "inverse:\n  name: missing\n  version: 9.9.9"
                    )
                    .replace("version: 1.0.0", "version: 2.0.0")
            )
            .unwrap_err()
            .to_string()
            .contains("not published")
        );
    }
}
