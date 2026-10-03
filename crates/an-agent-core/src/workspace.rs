//! Host software facts for this process. Not the file hot tree and not
//! the CAS store.
//!
//! `events` append. `config` is a new blob whenever the bytes change.
//! `env` is one markdown file, replaced wholesale, with a generation
//! event left on the log.

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceCite {
    pub config_id: Option<String>,
    pub env_id: Option<String>,
    pub env_markdown: String,
}

pub struct Workspace {
    root: PathBuf,
    lock: Mutex<()>,
}

impl Workspace {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, WorkspaceError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(root.join("config"))?;
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

    pub fn push_event(&self, body: &str) -> Result<WorkspaceRecord, WorkspaceError> {
        let _guard = self.lock.lock().unwrap_or_else(|err| err.into_inner());
        let record = WorkspaceRecord {
            id: Entropy::os().uuid_v4().to_string(),
            kind: "event".into(),
            body: Some(body.to_string()),
            sha256: None,
            at: None,
        };
        self.append(&record)?;
        Ok(record)
    }

    pub fn put_config(&self, bytes: &[u8]) -> Result<WorkspaceRecord, WorkspaceError> {
        let _guard = self.lock.lock().unwrap_or_else(|err| err.into_inner());
        let sha = sha256(bytes);
        if let Some(existing) = self.last_kind("config")?
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
        };
        self.append(&record)?;
        Ok(record)
    }

    pub fn put_env(&self, markdown: &str) -> Result<WorkspaceRecord, WorkspaceError> {
        let _guard = self.lock.lock().unwrap_or_else(|err| err.into_inner());
        let sha = sha256(markdown.as_bytes());
        if let Some(existing) = self.last_kind("env")?
            && existing.sha256.as_deref() == Some(sha.as_str())
        {
            return Ok(existing);
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
        };
        self.append(&record)?;
        Ok(record)
    }

    pub fn cite(&self) -> Result<WorkspaceCite, WorkspaceError> {
        let _guard = self.lock.lock().unwrap_or_else(|err| err.into_inner());
        let config_id = self.last_kind("config")?.map(|record| record.id);
        let env_id = self.last_kind("env")?.map(|record| record.id);
        let env_markdown = fs::read_to_string(self.root.join("env.md")).unwrap_or_default();
        Ok(WorkspaceCite {
            config_id,
            env_id,
            env_markdown,
        })
    }

    pub fn log(&self) -> Result<Vec<WorkspaceRecord>, WorkspaceError> {
        let _guard = self.lock.lock().unwrap_or_else(|err| err.into_inner());
        self.read_log()
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
        if sha256_hex.len() != 64 || !sha256_hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(WorkspaceError::UnknownConfig(sha256_hex.to_string()));
        }
        Ok(self.root.join("config").join(sha256_hex))
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
        let first = space.put_config(b"{\"model\":\"m\"}").unwrap();
        let again = space.put_config(b"{\"model\":\"m\"}").unwrap();
        assert_eq!(first.id, again.id);
        assert_eq!(space.log().unwrap().len(), 1);
        let second = space.put_config(b"{\"model\":\"n\"}").unwrap();
        assert_ne!(first.id, second.id);
        let old = first.sha256.clone().unwrap();
        assert_eq!(space.config_bytes(&old).unwrap(), b"{\"model\":\"m\"}");

        let env = space.put_env("# stage\n\nalpha\n").unwrap();
        assert!(env.at.is_some());
        let same = space.put_env("# stage\n\nalpha\n").unwrap();
        assert_eq!(env.id, same.id);
        let next = space.put_env("# stage\n\nbeta\n").unwrap();
        assert_ne!(env.id, next.id);
        assert_eq!(
            fs::read_to_string(tmp.path().join("workspace/env.md")).unwrap(),
            "# stage\n\nbeta\n"
        );
        let log = space.log().unwrap();
        assert_eq!(log.iter().filter(|record| record.kind == "env").count(), 2);

        let event = space.push_event("saved the buffer").unwrap();
        assert_eq!(event.kind, "event");
        assert_eq!(event.body.as_deref(), Some("saved the buffer"));
    }
}
