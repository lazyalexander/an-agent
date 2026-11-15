//! Append-only registry. A published body is addressed by its sha256.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
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
        sync_directory(&root)?;
        Ok(Self { root })
    }

    /// Store `yaml` if this name and version are new. The same bytes may
    /// be published again. A different body for the same version is refused,
    /// so an old workspace write can still name the descriptor that made it.
    pub fn publish(&self, yaml: &str) -> Result<SpoolSpec, SpoolError> {
        let spec = parse(yaml)?;
        let mut index = self.lock_index()?;
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
            // A previous append may have completed before its sync reported failure.
            index.sync_all()?;
            return Ok(existing);
        }
        let blob = self.blob_path(&spec.sha256);
        if !blob.exists() {
            self.write_blob(&blob, yaml.as_bytes())?;
        } else {
            self.recover_hash(&spec.sha256)?;
            File::open(&blob)?.sync_all()?;
            sync_directory(&self.root.join("blobs"))?;
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
        writeln!(index, "{}", serde_json::to_string(&line)?)?;
        index.sync_all()?;
        Ok(spec)
    }

    pub fn recover(&self, name: &str, version: &str) -> Result<SpoolSpec, SpoolError> {
        let _index = self.lock_index()?;
        self.lookup(name, version)?
            .ok_or_else(|| invalid(format!("spool not published: {name} {version}")))
    }

    pub fn recover_hash(&self, sha256: &str) -> Result<SpoolSpec, SpoolError> {
        if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(invalid("invalid registry blob hash"));
        }
        let body = fs::read_to_string(self.blob_path(sha256))?;
        let spec = parse(&body)?;
        if spec.sha256 != sha256 {
            return Err(invalid(format!("blob {sha256} does not hash to its name")));
        }
        Ok(spec)
    }

    /// Every published version of `name`, oldest first.
    pub fn versions(&self, name: &str) -> Result<Vec<SpoolSpec>, SpoolError> {
        let _index = self.lock_index()?;
        let mut out = Vec::new();
        for line in self.lines()? {
            if line.name == name {
                out.push(self.recover_line(&line)?);
            }
        }
        Ok(out)
    }

    fn lookup(&self, name: &str, version: &str) -> Result<Option<SpoolSpec>, SpoolError> {
        for line in self.lines()? {
            if line.name == name && line.version == version {
                return Ok(Some(self.recover_line(&line)?));
            }
        }
        Ok(None)
    }

    fn lines(&self) -> Result<Vec<IndexLine>, SpoolError> {
        let text = fs::read_to_string(self.index_path())?;
        if !text.is_empty() && !text.ends_with('\n') {
            return Err(invalid(
                "incomplete registry index tail; inspect before recovery",
            ));
        }
        let mut versions = BTreeMap::new();
        let mut out = Vec::new();
        for (i, line) in text.lines().enumerate() {
            if line.is_empty() {
                continue;
            }
            let entry: IndexLine = serde_json::from_str(line)
                .map_err(|e| invalid(format!("index line {}: {e}", i + 1)))?;
            let key = (entry.name.clone(), entry.version.clone());
            if let Some(hash) = versions.insert(key, entry.sha256.clone()) {
                if hash != entry.sha256 {
                    return Err(invalid("conflicting immutable versions in registry index"));
                }
                continue;
            }
            out.push(entry);
        }
        Ok(out)
    }

    fn recover_line(&self, line: &IndexLine) -> Result<SpoolSpec, SpoolError> {
        let spec = self.recover_hash(&line.sha256)?;
        if spec.name != line.name || spec.version != line.version {
            return Err(invalid("registry index identity differs from its blob"));
        }
        Ok(spec)
    }

    // Lock the append-only index inode itself: separate Registry instances and
    // cooperating processes serialize checks and commit without a lockfile lease.
    fn lock_index(&self) -> Result<File, SpoolError> {
        let file = OpenOptions::new()
            .read(true)
            .append(true)
            .open(self.index_path())?;
        file.lock()?;
        Ok(file)
    }

    fn write_blob(&self, blob: &Path, bytes: &[u8]) -> Result<(), SpoolError> {
        let id = an_agent_core::det_seam::Entropy::os().uuid_v4();
        let tmp = self.root.join("blobs").join(format!("{id}.pending"));
        let mut created = false;
        let result = (|| -> Result<(), SpoolError> {
            let mut file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
            created = true;
            file.write_all(bytes)?;
            file.sync_all()?;
            fs::rename(&tmp, blob)?;
            sync_directory(&self.root.join("blobs"))?;
            Ok(())
        })();
        if created && result.is_err() {
            let _ = fs::remove_file(tmp);
        }
        result
    }

    fn index_path(&self) -> PathBuf {
        self.root.join("index.jsonl")
    }

    fn blob_path(&self, sha256: &str) -> PathBuf {
        self.root.join("blobs").join(sha256)
    }
}

fn sync_directory(path: &Path) -> Result<(), SpoolError> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
