//! Append-only path log for one worker session. Old blob bytes are never
//! rewritten. This is not the workplace CAS.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::act::{FileFacet, Permit};

#[derive(Debug, Error)]
pub enum SubWpError {
    #[error("grant denies this sub-workplace op")]
    Denied,
    #[error("path not found: {0}")]
    NotFound(String),
    #[error("path escapes the sub-workplace: {0}")]
    BadPath(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("log: {0}")]
    Log(String),
}

#[derive(Clone, Copy)]
enum Op {
    Read,
    Write,
}

pub struct SubWp {
    root: PathBuf,
    lock: Mutex<()>,
}

impl SubWp {
    pub fn open(session_root: &Path) -> Result<Self, SubWpError> {
        let root = session_root.join("subwp");
        fs::create_dir_all(root.join("blobs"))?;
        let log = root.join("log.jsonl");
        if !log.exists() {
            fs::write(&log, b"")?;
        }
        Ok(Self {
            root,
            lock: Mutex::new(()),
        })
    }

    pub fn append(
        &self,
        path: &str,
        bytes: &[u8],
        permit: Permit,
        face: &FileFacet,
    ) -> Result<(), SubWpError> {
        let path = clean_path(path)?;
        allow(Op::Write, &path, permit, face)?;
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let sha = write_blob(&self.root, bytes)?;
        self.append_line(&json!({ "op": "write", "path": path, "sha256": sha }))
    }

    pub fn tombstone(
        &self,
        path: &str,
        permit: Permit,
        face: &FileFacet,
    ) -> Result<(), SubWpError> {
        let path = clean_path(path)?;
        allow(Op::Write, &path, permit, face)?;
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        self.append_line(&json!({ "op": "tombstone", "path": path }))
    }

    pub fn read(
        &self,
        path: &str,
        permit: Permit,
        face: &FileFacet,
    ) -> Result<Vec<u8>, SubWpError> {
        let path = clean_path(path)?;
        allow(Op::Read, &path, permit, face)?;
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let sha = self.live_sha(&path)?;
        fs::read(self.root.join("blobs").join(sha)).map_err(SubWpError::from)
    }

    pub fn snapshot_hash(&self) -> Result<String, SubWpError> {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let bytes = fs::read(self.root.join("log.jsonl"))?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }

    fn live_sha(&self, path: &str) -> Result<String, SubWpError> {
        let text = fs::read_to_string(self.root.join("log.jsonl"))?;
        for line in text.lines().rev() {
            if line.is_empty() {
                continue;
            }
            let v: Value =
                serde_json::from_str(line).map_err(|e| SubWpError::Log(e.to_string()))?;
            if v["path"].as_str() != Some(path) {
                continue;
            }
            return match v["op"].as_str() {
                Some("write") => v["sha256"]
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| SubWpError::Log("write missing sha256".into())),
                Some("tombstone") => Err(SubWpError::NotFound(path.to_string())),
                _ => Err(SubWpError::Log("unknown op".into())),
            };
        }
        Err(SubWpError::NotFound(path.to_string()))
    }

    fn append_line(&self, value: &Value) -> Result<(), SubWpError> {
        let mut file = OpenOptions::new()
            .append(true)
            .open(self.root.join("log.jsonl"))?;
        writeln!(file, "{value}")?;
        file.sync_all()?;
        Ok(())
    }
}

fn write_blob(root: &Path, bytes: &[u8]) -> Result<String, SubWpError> {
    let sha = format!("{:x}", Sha256::digest(bytes));
    let path = root.join("blobs").join(&sha);
    if !path.exists() {
        let tmp = root.join("blobs").join(format!("{sha}.tmp"));
        fs::write(&tmp, bytes)?;
        fs::rename(tmp, path)?;
    }
    Ok(sha)
}

fn clean_path(path: &str) -> Result<String, SubWpError> {
    if path.is_empty() || path.starts_with('/') || path.contains('\\') {
        return Err(SubWpError::BadPath(path.into()));
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(SubWpError::BadPath(path.into()));
        }
        parts.push(part);
    }
    Ok(parts.join("/"))
}

fn allow(op: Op, path: &str, permit: Permit, face: &FileFacet) -> Result<(), SubWpError> {
    if permit == Permit::Deny {
        return Err(SubWpError::Denied);
    }
    let ok = match face {
        FileFacet::None => false,
        FileFacet::Unbounded => true,
        FileFacet::Read { path: root, .. } => matches!(op, Op::Read) && under(root, path),
        FileFacet::Write { path: root, .. } => matches!(op, Op::Write) && under(root, path),
        FileFacet::ReadWrite { path: root, .. } => under(root, path),
    };
    if ok { Ok(()) } else { Err(SubWpError::Denied) }
}

fn under(root: &str, path: &str) -> bool {
    path == root || path.starts_with(&format!("{root}/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::act::Permit;
    use crate::act::{ActSentence, BareFile, FileFacet, Ingest, MemoryFacet, ToolTag};
    use crate::instance::{ProductPtr, Steward};
    use crate::principal::card::{AgentCard, ModelSpec, ToolGrant, Topology};
    use crate::testkit::TempDir;
    use uuid::Uuid;

    fn face_rw() -> FileFacet {
        FileFacet::ReadWrite {
            path: "src".into(),
            recursive: true,
        }
    }

    fn card(id: &str, permit: Permit) -> AgentCard {
        AgentCard {
            v: 1,
            id: Uuid::parse_str(id).unwrap(),
            model: ModelSpec {
                base_url: "https://example.com".into(),
                model: "m".into(),
                extra_body: None,
            },
            prompt: "p".into(),
            tools: vec![ToolGrant {
                name: "bash".into(),
                tag: ToolTag {
                    file: FileFacet::None,
                    permit,
                    memory: MemoryFacet::Ignore,
                },
            }],
            topology: Topology::Leaf,
            kernel: "0.1.0".into(),
            supersedes: None,
        }
    }

    #[test]
    fn append_tombstone_keeps_old_bytes_and_links_hash() {
        let tmp = TempDir::new("subwp");
        let steward = Steward::open(
            &card("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", Permit::Deny),
            tmp.path(),
            "{}",
            "[]",
        )
        .unwrap();
        let worker = steward
            .spawn_worker(
                &card("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb", Permit::Go),
                &ActSentence::bare(Permit::Go, BareFile::None, Ingest::Ignore),
            )
            .unwrap();
        let log = SubWp::open(worker.session().root()).unwrap();
        log.append("src/a.txt", b"v1", Permit::Go, &face_rw())
            .unwrap();
        let first = format!("{:x}", Sha256::digest(b"v1"));
        log.append("src/a.txt", b"v2", Permit::Go, &face_rw())
            .unwrap();
        assert_eq!(
            log.read("src/a.txt", Permit::Go, &face_rw()).unwrap(),
            b"v2"
        );
        let blob = worker.session().root().join("subwp/blobs").join(&first);
        assert_eq!(fs::read(&blob).unwrap(), b"v1");
        log.tombstone("src/a.txt", Permit::Go, &face_rw()).unwrap();
        assert!(matches!(
            log.read("src/a.txt", Permit::Go, &face_rw()),
            Err(SubWpError::NotFound(_))
        ));
        assert_eq!(fs::read(blob).unwrap(), b"v1");
        assert!(matches!(
            log.append("src/a.txt", b"no", Permit::Deny, &face_rw()),
            Err(SubWpError::Denied)
        ));
        let digest = log.snapshot_hash().unwrap();
        let id = steward
            .link(&ProductPtr {
                child_session: worker.session().id_str().into(),
                tape_sha256: "tape".into(),
                md_sha256: None,
                tree_id: None,
                subwp_sha256: Some(digest.clone()),
            })
            .unwrap();
        let events = steward.agent().session().tape().read_all().unwrap();
        let link = events.iter().find(|e| e.id == id).unwrap();
        assert!(link.content.contains(&digest));
        assert!(!link.content.contains("v1"));
        assert!(!link.content.contains("v2"));
    }
}
