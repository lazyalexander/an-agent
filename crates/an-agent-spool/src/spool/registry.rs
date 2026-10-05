//! Append-only registry. A published body is addressed by its sha256.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{Inverse, SpoolError, SpoolSpec, invalid, parse};

/// Append-only registry of spool bodies. `blobs/<sha256>` is the YAML.
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
    Spool { name: String, version: String },
}

impl Registry {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, SpoolError> {
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
    pub fn publish(&self, yaml: &str) -> Result<SpoolSpec, SpoolError> {
        let spec = parse(yaml)?;
        for req in &spec.requires {
            if self.lookup(&req.name, &req.version)?.is_none() {
                return Err(invalid(format!(
                    "required spool is not published: {} {}",
                    req.name, req.version
                )));
            }
        }
        if let Inverse::Spool { name, version } = &spec.inverse
            && self.lookup(name, version)?.is_none()
        {
            return Err(invalid(format!(
                "inverse spool is not published: {name} {version}"
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
                Inverse::None => IndexInverse::Irreversible("none".into()),
                Inverse::Irreversible => IndexInverse::Irreversible("irreversible".into()),
                Inverse::Spool { name, version } => IndexInverse::Spool {
                    name: name.clone(),
                    version: version.clone(),
                },
            },
        };
        let mut index = OpenOptions::new().append(true).open(self.index_path())?;
        writeln!(index, "{}", serde_json::to_string(&line)?)?;
        Ok(spec)
    }

    pub fn recover(&self, name: &str, version: &str) -> Result<SpoolSpec, SpoolError> {
        self.lookup(name, version)?
            .ok_or_else(|| invalid(format!("spool not published: {name} {version}")))
    }

    pub fn recover_hash(&self, sha256: &str) -> Result<SpoolSpec, SpoolError> {
        let body = fs::read_to_string(self.blob_path(sha256))?;
        let spec = parse(&body)?;
        if spec.sha256 != sha256 {
            return Err(invalid(format!("blob {sha256} does not hash to its name")));
        }
        Ok(spec)
    }

    /// Every published version of `name`, oldest first.
    pub fn versions(&self, name: &str) -> Result<Vec<SpoolSpec>, SpoolError> {
        let mut out = Vec::new();
        for line in self.lines()? {
            if line.name == name {
                out.push(self.recover_hash(&line.sha256)?);
            }
        }
        Ok(out)
    }

    fn lookup(&self, name: &str, version: &str) -> Result<Option<SpoolSpec>, SpoolError> {
        for line in self.lines()? {
            if line.name == name && line.version == version {
                return Ok(Some(self.recover_hash(&line.sha256)?));
            }
        }
        Ok(None)
    }

    fn lines(&self) -> Result<Vec<IndexLine>, SpoolError> {
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
