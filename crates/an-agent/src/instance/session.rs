//! Private domain of one agent run: tape, files, scratch. Not a shared pool.

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;
use uuid::Uuid;

use crate::det_seam::Entropy;
use crate::memstream::{JsonlStore, StoreError};

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("store: {0}")]
    Store(#[from] StoreError),
}

/// One engagement's private world. Layout:
/// `<root>/<agent-id>/<session-id>/{memory.jsonl,files/,scratch/}`
pub struct Session {
    id: Uuid,
    id_str: String,
    agent_id: Uuid,
    agent_id_str: String,
    root: PathBuf,
    tape: JsonlStore,
}

impl Session {
    pub fn create(root: impl AsRef<Path>, agent_id: Uuid) -> Result<Self, SessionError> {
        let mut entropy = Entropy::os();
        let id = entropy.uuid_v4();
        Self::open_at(root.as_ref(), agent_id, id, true)
    }

    pub fn open(root: impl AsRef<Path>, agent_id: Uuid, id: Uuid) -> Result<Self, SessionError> {
        Self::open_at(root.as_ref(), agent_id, id, false)
    }

    fn open_at(root: &Path, agent_id: Uuid, id: Uuid, create: bool) -> Result<Self, SessionError> {
        let dir = root.join(agent_id.to_string()).join(id.to_string());
        if create {
            fs::create_dir_all(dir.join("files"))?;
            fs::create_dir_all(dir.join("scratch"))?;
        }
        let tape = JsonlStore::open(dir.join("memory.jsonl"))?;
        Ok(Self {
            id,
            id_str: id.to_string(),
            agent_id,
            agent_id_str: agent_id.to_string(),
            root: dir,
            tape,
        })
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn id_str(&self) -> &str {
        &self.id_str
    }

    pub fn agent_id(&self) -> Uuid {
        self.agent_id
    }

    pub fn agent_id_str(&self) -> &str {
        &self.agent_id_str
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn files(&self) -> PathBuf {
        self.root.join("files")
    }

    pub fn scratch(&self) -> PathBuf {
        self.root.join("scratch")
    }

    pub fn tape(&self) -> &JsonlStore {
        &self.tape
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memstream::{AppendEvent, FromKind, Kind};
    use crate::testkit::TempDir;

    #[test]
    fn create_lays_out_private_dirs_and_tape() {
        let tmp = TempDir::new("session");
        let agent = Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap();
        let session = Session::create(tmp.path(), agent).unwrap();
        assert!(session.files().is_dir());
        assert!(session.scratch().is_dir());
        session
            .tape()
            .append(AppendEvent {
                from: agent.to_string(),
                from_kind: FromKind::Agent,
                kind: Kind::Utterance,
                session: session.id_str().into(),
                content: "hi".into(),
                tags: vec![],
                refs: vec![],
                act: None,
                card: None,
            })
            .unwrap();
        assert_eq!(session.tape().read_all().unwrap().len(), 1);
        let reopened = Session::open(tmp.path(), agent, session.id()).unwrap();
        assert_eq!(reopened.tape().read_all().unwrap().len(), 1);
    }
}
