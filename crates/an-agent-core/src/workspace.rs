//! Host software facts for this process. Not the file hot tree and not
//! the CAS store.
//!
//! `events` append. `config` is a JSON document the host injects, stored
//! as a new blob whenever the bytes change. `env` is markdown, one file
//! replaced wholesale, with a generation event left on the log.
//! `register` is one content-addressed snapshot of the names a plugin
//! declared. Parsing of config stays on the host face.

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::det_seam::{Clock, Entropy};

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unknown config snapshot: {0}")]
    UnknownConfig(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceRecord {
    pub id: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    /// Plugin name for this fact. Absent on older log lines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceCite {
    pub config_id: Option<String>,
    pub config_sha256: Option<String>,
    pub env_id: Option<String>,
    pub env_sha256: Option<String>,
    pub env_markdown: String,
    pub register_id: Option<String>,
    pub register_sha256: Option<String>,
}

pub struct Workspace {
    root: PathBuf,
    lock: Mutex<()>,
}

impl Workspace {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, WorkspaceError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(root.join("config"))?;
        fs::create_dir_all(root.join("env"))?;
        fs::create_dir_all(root.join("register"))?;
        if !root.join("env.md").exists() {
            fs::write(root.join("env.md"), b"")?;
        }
        if !root.join("log.jsonl").exists() {
            fs::write(root.join("log.jsonl"), b"")?;
        }
        Ok(Self {
            root,
            lock: Mutex::new(()),
        })
    }

    pub fn push_event(&self, name: &str, body: &str) -> Result<WorkspaceRecord, WorkspaceError> {
        let _guard = self.lock.lock().unwrap_or_else(|err| err.into_inner());
        let record = WorkspaceRecord {
            id: Entropy::os().uuid_v4().to_string(),
            kind: "event".into(),
            body: Some(body.to_string()),
            sha256: None,
            at: None,
            name: Some(name.to_string()),
        };
        self.append(&record)?;
        Ok(record)
    }

    pub fn put_config(&self, name: &str, bytes: &[u8]) -> Result<WorkspaceRecord, WorkspaceError> {
        let _guard = self.lock.lock().unwrap_or_else(|err| err.into_inner());
        let sha = sha256(bytes);
        if let Some(existing) = self.last_named("config", name)?
            && existing.sha256.as_deref() == Some(sha.as_str())
        {
            return Ok(existing);
        }
        let path = self.config_path(&sha)?;
        if !path.exists() {
            fs::write(&path, bytes)?;
        }
        let record = WorkspaceRecord {
            id: Entropy::os().uuid_v4().to_string(),
            kind: "config".into(),
            body: None,
            sha256: Some(sha),
            at: None,
            name: Some(name.to_string()),
        };
        self.append(&record)?;
        Ok(record)
    }

    pub fn put_env(&self, name: &str, markdown: &str) -> Result<WorkspaceRecord, WorkspaceError> {
        let _guard = self.lock.lock().unwrap_or_else(|err| err.into_inner());
        let sha = sha256(markdown.as_bytes());
        if let Some(existing) = self.last_named("env", name)?
            && existing.sha256.as_deref() == Some(sha.as_str())
        {
            return Ok(existing);
        }
        let blob = self.env_path(&sha)?;
        if !blob.exists() {
            fs::write(&blob, markdown.as_bytes())?;
        }
        let tmp = self.root.join("env.md.tmp");
        fs::write(&tmp, markdown.as_bytes())?;
        fs::rename(&tmp, self.root.join("env.md"))?;
        let record = WorkspaceRecord {
            id: Entropy::os().uuid_v4().to_string(),
            kind: "env".into(),
            body: None,
            sha256: Some(sha),
            at: Some(Clock::wall().now_iso()),
            name: Some(name.to_string()),
        };
        self.append(&record)?;
        Ok(record)
    }

    /// Store one registration document. Identical bytes keep the previous id.
    pub fn put_register(&self, bytes: &[u8]) -> Result<WorkspaceRecord, WorkspaceError> {
        let _guard = self.lock.lock().unwrap_or_else(|err| err.into_inner());
        let sha = sha256(bytes);
        if let Some(existing) = self.last_kind("register")?
            && existing.sha256.as_deref() == Some(sha.as_str())
        {
            return Ok(existing);
        }
        let path = self.register_path(&sha)?;
        if !path.exists() {
            fs::write(&path, bytes)?;
        }
        let record = WorkspaceRecord {
            id: Entropy::os().uuid_v4().to_string(),
            kind: "register".into(),
            body: None,
            sha256: Some(sha),
            at: None,
            name: None,
        };
        self.append(&record)?;
        Ok(record)
    }

    pub fn register_bytes(&self, sha256_hex: &str) -> Result<Vec<u8>, WorkspaceError> {
        let path = self.register_path(sha256_hex)?;
        if !path.exists() {
            return Err(WorkspaceError::UnknownConfig(sha256_hex.to_string()));
        }
        Ok(fs::read(path)?)
    }

    pub fn cite(&self) -> Result<WorkspaceCite, WorkspaceError> {
        let _guard = self.lock.lock().unwrap_or_else(|err| err.into_inner());
        let config = self.last_kind("config")?;
        let env = self.last_kind("env")?;
        let register = self.last_kind("register")?;
        let env_markdown = fs::read_to_string(self.root.join("env.md")).unwrap_or_default();
        Ok(WorkspaceCite {
            config_id: config.as_ref().map(|record| record.id.clone()),
            config_sha256: config.and_then(|record| record.sha256),
            env_id: env.as_ref().map(|record| record.id.clone()),
            env_sha256: env.and_then(|record| record.sha256),
            env_markdown,
            register_id: register.as_ref().map(|record| record.id.clone()),
            register_sha256: register.and_then(|record| record.sha256),
        })
    }

    pub fn log(&self) -> Result<Vec<WorkspaceRecord>, WorkspaceError> {
        let _guard = self.lock.lock().unwrap_or_else(|err| err.into_inner());
        self.read_log()
    }

    pub fn env_bytes(&self, sha256_hex: &str) -> Result<Vec<u8>, WorkspaceError> {
        let path = self.env_path(sha256_hex)?;
        if !path.exists() {
            return Err(WorkspaceError::UnknownConfig(sha256_hex.to_string()));
        }
        Ok(fs::read(path)?)
    }

    pub fn config_bytes(&self, sha256_hex: &str) -> Result<Vec<u8>, WorkspaceError> {
        let path = self.config_path(sha256_hex)?;
        if !path.exists() {
            return Err(WorkspaceError::UnknownConfig(sha256_hex.to_string()));
        }
        Ok(fs::read(path)?)
    }

    fn append(&self, record: &WorkspaceRecord) -> Result<(), WorkspaceError> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.root.join("log.jsonl"))?;
        writeln!(file, "{}", serde_json::to_string(record)?)?;
        file.sync_data()?;
        Ok(())
    }

    fn last_kind(&self, kind: &str) -> Result<Option<WorkspaceRecord>, WorkspaceError> {
        Ok(self
            .read_log()?
            .into_iter()
            .rev()
            .find(|record| record.kind == kind))
    }

    fn read_log(&self) -> Result<Vec<WorkspaceRecord>, WorkspaceError> {
        let file = OpenOptions::new()
            .read(true)
            .open(self.root.join("log.jsonl"))?;
        let mut out = Vec::new();
        for line in BufReader::new(file).lines() {
            let line = line?;
            if line.is_empty() {
                continue;
            }
            out.push(serde_json::from_str(&line)?);
        }
        Ok(out)
    }

    fn config_path(&self, sha256_hex: &str) -> Result<PathBuf, WorkspaceError> {
        Ok(self.root.join("config").join(self.hex_name(sha256_hex)?))
    }

    fn env_path(&self, sha256_hex: &str) -> Result<PathBuf, WorkspaceError> {
        Ok(self.root.join("env").join(self.hex_name(sha256_hex)?))
    }

    fn register_path(&self, sha256_hex: &str) -> Result<PathBuf, WorkspaceError> {
        Ok(self.root.join("register").join(self.hex_name(sha256_hex)?))
    }

    fn last_named(
        &self,
        kind: &str,
        name: &str,
    ) -> Result<Option<WorkspaceRecord>, WorkspaceError> {
        Ok(self
            .read_log()?
            .into_iter()
            .rev()
            .find(|record| record.kind == kind && record.name.as_deref() == Some(name)))
    }

    fn hex_name(&self, sha256_hex: &str) -> Result<String, WorkspaceError> {
        if sha256_hex.len() != 64 || !sha256_hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(WorkspaceError::UnknownConfig(sha256_hex.to_string()));
        }
        Ok(sha256_hex.to_string())
    }
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::TempDir;

    #[test]
    fn config_is_addressed_and_env_is_replaced() {
        let tmp = TempDir::new("workspace-facts");
        let space = Workspace::open(tmp.path().join("workspace")).unwrap();
        let first = space.put_config("canvas", b"{\"model\":\"m\"}").unwrap();
        let again = space.put_config("canvas", b"{\"model\":\"m\"}").unwrap();
        assert_eq!(first.id, again.id);
        assert_eq!(space.log().unwrap().len(), 1);
        let second = space.put_config("canvas", b"{\"model\":\"n\"}").unwrap();
        assert_ne!(first.id, second.id);
        let old = first.sha256.clone().unwrap();
        assert_eq!(space.config_bytes(&old).unwrap(), b"{\"model\":\"m\"}");

        let env = space.put_env("stage", "# stage\n\nalpha\n").unwrap();
        assert!(env.at.is_some());
        let same = space.put_env("stage", "# stage\n\nalpha\n").unwrap();
        assert_eq!(env.id, same.id);
        let next = space.put_env("stage", "# stage\n\nbeta\n").unwrap();
        assert_ne!(env.id, next.id);
        assert_eq!(
            fs::read_to_string(tmp.path().join("workspace/env.md")).unwrap(),
            "# stage\n\nbeta\n"
        );
        let old_env = env.sha256.clone().unwrap();
        assert_eq!(space.env_bytes(&old_env).unwrap(), b"# stage\n\nalpha\n");
        let log = space.log().unwrap();
        assert_eq!(log.iter().filter(|record| record.kind == "env").count(), 2);

        let event = space.push_event("document", "saved the buffer").unwrap();
        assert_eq!(event.name.as_deref(), Some("document"));
        assert_eq!(event.kind, "event");
        assert_eq!(event.body.as_deref(), Some("saved the buffer"));

        let listed = br#"{"events":[],"config":["canvas"],"env":["stage"]}"#;
        let register = space.put_register(listed).unwrap();
        let again = space.put_register(listed).unwrap();
        assert_eq!(register.id, again.id);
        let moved = br#"{"events":[],"config":["canvas","ink"],"env":["stage"]}"#;
        let next = space.put_register(moved).unwrap();
        assert_ne!(register.id, next.id);
        let old = register.sha256.clone().unwrap();
        assert_eq!(space.register_bytes(&old).unwrap(), listed);
        assert_eq!(
            space.cite().unwrap().register_id.as_deref(),
            Some(next.id.as_str())
        );
    }

    #[test]
    fn an_old_log_line_without_a_name_still_reads() {
        let record: WorkspaceRecord =
            serde_json::from_str(r#"{"id":"1","kind":"event","body":"saved"}"#).unwrap();
        assert_eq!(record.name, None);
        assert_eq!(record.body.as_deref(), Some("saved"));
    }
}
