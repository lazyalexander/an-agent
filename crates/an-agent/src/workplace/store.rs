use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use super::id::ObjectId;
use super::resource::{Resource, ResourceKind};

#[derive(Debug, Error)]
pub enum WorkplaceError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Msg(String),
    #[error("only the lead agent may change this workplace")]
    NotLead,
    #[error("unknown ref {0}")]
    UnknownRef(String),
    #[error("not a tree object")]
    NotATree,
}

/// Governance files in the tree. Suffix is `.wp`, not `.json`.
pub const WORKERS_FILE: &[&str] = &[".workplace", "workers.wp"];
pub const PERMIT_FILE: &[&str] = &[".workplace", "permit.wp"];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Meta {
    lead: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeEntry {
    pub kind: ResourceKind,
    pub id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tree {
    pub entries: BTreeMap<String, TreeEntry>,
}

#[derive(Debug, Clone)]
pub struct Commit {
    pub from_head: ObjectId,
    pub to_head: ObjectId,
    pub blob: ObjectId,
}

pub struct Workplace {
    pub id: Uuid,
    root: PathBuf,
    lead: Uuid,
}

impl Workplace {
    pub fn create(dir: impl AsRef<Path>, lead: Uuid) -> Result<Self, WorkplaceError> {
        let id = crate::det_seam::Entropy::os().uuid_v4();
        let root = dir.as_ref().join(id.to_string());
        fs::create_dir_all(root.join("objects"))?;
        fs::create_dir_all(root.join("refs"))?;
        let wp = Self {
            id,
            root: root.clone(),
            lead,
        };
        wp.write_meta(&Meta { lead, branch: None })?;
        let empty = wp.put_tree(&Tree::default())?;
        fs::write(root.join("HEAD"), format!("{empty}\n"))?;
        Ok(wp)
    }

    pub fn open(root: impl AsRef<Path>, id: Uuid) -> Result<Self, WorkplaceError> {
        let root = root.as_ref().join(id.to_string());
        let meta: Meta = serde_json::from_slice(&fs::read(root.join("meta.wp"))?)?;
        Ok(Self {
            id,
            root,
            lead: meta.lead,
        })
    }

    pub fn lead(&self) -> Uuid {
        self.lead
    }

    pub fn head(&self) -> Result<ObjectId, WorkplaceError> {
        let raw = fs::read_to_string(self.root.join("HEAD"))?;
        ObjectId::from_hex(raw.trim()).map_err(WorkplaceError::Msg)
    }

    pub fn resource(&self, kind: ResourceKind, path: impl IntoIterator<Item = impl Into<String>>) -> Resource {
        Resource::new(self.id, kind, path)
    }

    pub fn put_blob(&self, bytes: &[u8]) -> Result<ObjectId, WorkplaceError> {
        let id = ObjectId::from_payload("blob", bytes);
        self.write_object(&id, bytes)?;
        Ok(id)
    }

    pub fn get_blob(&self, id: &ObjectId) -> Result<Vec<u8>, WorkplaceError> {
        fs::read(self.object_path(id)).map_err(Into::into)
    }

    pub fn put_tree(&self, tree: &Tree) -> Result<ObjectId, WorkplaceError> {
        let payload = serde_json::to_vec(tree)?;
        let id = ObjectId::from_payload("tree", &payload);
        self.write_object(&id, &payload)?;
        Ok(id)
    }

    pub fn get_tree(&self, id: &ObjectId) -> Result<Tree, WorkplaceError> {
        let bytes = fs::read(self.object_path(id))?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn read_file(&self, path: &[String]) -> Result<Option<Vec<u8>>, WorkplaceError> {
        let Some(id) = self.lookup(path)? else {
            return Ok(None);
        };
        Ok(Some(self.get_blob(&id)?))
    }

    /// Replace a file at `path`. Advances HEAD. Unchanged blobs keep their ids.
    pub fn write_file(
        &self,
        caller: Uuid,
        path: &[String],
        bytes: &[u8],
    ) -> Result<Commit, WorkplaceError> {
        if caller != self.lead {
            return Err(WorkplaceError::NotLead);
        }
        if path.is_empty() {
            return Err(WorkplaceError::Msg("file path is empty".into()));
        }
        let from_head = self.head()?;
        let blob = self.put_blob(bytes)?;
        let root = self.get_tree(&from_head)?;
        let new_root = self.upsert(root, path, ResourceKind::File, blob)?;
        let to_head = self.put_tree(&new_root)?;
        self.set_head(to_head)?;
        Ok(Commit {
            from_head,
            to_head,
            blob,
        })
    }

    pub fn set_workers(&self, caller: Uuid, body: &[u8]) -> Result<Commit, WorkplaceError> {
        self.write_file(caller, &owned(WORKERS_FILE), body)
    }

    pub fn set_permit(&self, caller: Uuid, body: &[u8]) -> Result<Commit, WorkplaceError> {
        self.write_file(caller, &owned(PERMIT_FILE), body)
    }

    pub fn workers(&self) -> Result<Option<Vec<u8>>, WorkplaceError> {
        self.read_file(&owned(WORKERS_FILE))
    }

    pub fn permit(&self) -> Result<Option<Vec<u8>>, WorkplaceError> {
        self.read_file(&owned(PERMIT_FILE))
    }

    /// Point a private branch at the current HEAD.
    pub fn branch(&self, caller: Uuid, name: &str) -> Result<ObjectId, WorkplaceError> {
        self.require_lead(caller)?;
        validate_ref(name)?;
        let head = self.head()?;
        self.write_ref(name, head)?;
        Ok(head)
    }

    pub fn checkout(&self, caller: Uuid, name: &str) -> Result<ObjectId, WorkplaceError> {
        self.require_lead(caller)?;
        let id = self.read_ref(name)?;
        self.get_tree(&id).map_err(|_| WorkplaceError::NotATree)?;
        self.set_head_and_branch(id, Some(name.to_string()))?;
        Ok(id)
    }

    /// Rollback HEAD (and the current branch, if any) to an existing tree.
    pub fn reset(&self, caller: Uuid, tree: ObjectId) -> Result<ObjectId, WorkplaceError> {
        self.require_lead(caller)?;
        self.get_tree(&tree).map_err(|_| WorkplaceError::NotATree)?;
        self.set_head(tree)?;
        Ok(tree)
    }

    /// Fast-forward HEAD to a named ref. No three-way merge.
    pub fn merge_ff(&self, caller: Uuid, name: &str) -> Result<ObjectId, WorkplaceError> {
        self.require_lead(caller)?;
        let id = self.read_ref(name)?;
        self.get_tree(&id).map_err(|_| WorkplaceError::NotATree)?;
        self.set_head(id)?;
        Ok(id)
    }

    fn require_lead(&self, caller: Uuid) -> Result<(), WorkplaceError> {
        if caller != self.lead {
            Err(WorkplaceError::NotLead)
        } else {
            Ok(())
        }
    }

    fn read_meta(&self) -> Result<Meta, WorkplaceError> {
        Ok(serde_json::from_slice(&fs::read(self.root.join("meta.wp"))?)?)
    }

    fn write_meta(&self, meta: &Meta) -> Result<(), WorkplaceError> {
        fs::write(self.root.join("meta.wp"), serde_json::to_vec_pretty(meta)?)?;
        Ok(())
    }

    fn set_head(&self, id: ObjectId) -> Result<(), WorkplaceError> {
        let branch = self.read_meta()?.branch;
        self.set_head_and_branch(id, branch)
    }

    fn set_head_and_branch(
        &self,
        id: ObjectId,
        branch: Option<String>,
    ) -> Result<(), WorkplaceError> {
        fs::write(self.root.join("HEAD"), format!("{id}\n"))?;
        if let Some(name) = &branch {
            self.write_ref(name, id)?;
        }
        let mut meta = self.read_meta()?;
        meta.branch = branch;
        self.write_meta(&meta)
    }

    fn ref_path(&self, name: &str) -> PathBuf {
        self.root.join("refs").join(name)
    }

    fn write_ref(&self, name: &str, id: ObjectId) -> Result<(), WorkplaceError> {
        fs::create_dir_all(self.root.join("refs"))?;
        fs::write(self.ref_path(name), format!("{id}\n"))?;
        Ok(())
    }

    fn read_ref(&self, name: &str) -> Result<ObjectId, WorkplaceError> {
        let path = self.ref_path(name);
        if !path.exists() {
            return Err(WorkplaceError::UnknownRef(name.into()));
        }
        let raw = fs::read_to_string(path)?;
        ObjectId::from_hex(raw.trim()).map_err(WorkplaceError::Msg)
    }

    fn lookup(&self, path: &[String]) -> Result<Option<ObjectId>, WorkplaceError> {
        let mut tree = self.get_tree(&self.head()?)?;
        if path.is_empty() {
            return Ok(Some(self.head()?));
        }
        for (i, name) in path.iter().enumerate() {
            let Some(entry) = tree.entries.get(name) else {
                return Ok(None);
            };
            let id = ObjectId::from_hex(&entry.id).map_err(WorkplaceError::Msg)?;
            if i + 1 == path.len() {
                return Ok(Some(id));
            }
            if entry.kind != ResourceKind::Tree {
                return Err(WorkplaceError::Msg("path prefix is not a tree".into()));
            }
            tree = self.get_tree(&id)?;
        }
        Ok(None)
    }

    fn upsert(
        &self,
        mut tree: Tree,
        path: &[String],
        kind: ResourceKind,
        id: ObjectId,
    ) -> Result<Tree, WorkplaceError> {
        let name = &path[0];
        if path.len() == 1 {
            tree.entries.insert(
                name.clone(),
                TreeEntry {
                    kind,
                    id: id.to_string(),
                },
            );
            return Ok(tree);
        }
        let child = match tree.entries.get(name) {
            Some(e) if e.kind == ResourceKind::Tree => {
                let cid = ObjectId::from_hex(&e.id).map_err(WorkplaceError::Msg)?;
                self.get_tree(&cid)?
            }
            Some(_) => return Err(WorkplaceError::Msg("cannot nest under a file".into())),
            None => Tree::default(),
        };
        let child = self.upsert(child, &path[1..], kind, id)?;
        let cid = self.put_tree(&child)?;
        tree.entries.insert(
            name.clone(),
            TreeEntry {
                kind: ResourceKind::Tree,
                id: cid.to_string(),
            },
        );
        Ok(tree)
    }

    fn object_path(&self, id: &ObjectId) -> PathBuf {
        let h = id.hex();
        self.root.join("objects").join(&h[..2]).join(&h[2..])
    }

    fn write_object(&self, id: &ObjectId, bytes: &[u8]) -> Result<(), WorkplaceError> {
        let path = self.object_path(id);
        if path.exists() {
            return Ok(());
        }
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("tmp");
        {
            let mut f = File::create(&tmp)?;
            f.write_all(bytes)?;
        }
        fs::rename(tmp, path)?;
        Ok(())
    }
}

fn owned(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| (*s).to_string()).collect()
}

fn validate_ref(name: &str) -> Result<(), WorkplaceError> {
    if name.is_empty()
        || name.contains('/')
        || name.contains("..")
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(WorkplaceError::Msg("invalid ref name".into()));
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use crate::testkit::TempDir;

    fn tmp() -> TempDir {
        TempDir::new("wp")
    }

    #[test]
    fn write_advances_head_and_shares_blobs() {
        let lead = Uuid::new_v4();
        let other = Uuid::new_v4();
        let tmp = tmp();
        let wp = Workplace::create(tmp.path(), lead).unwrap();
        let empty = wp.head().unwrap();
        let c1 = wp
            .write_file(lead, &["src".into(), "a.txt".into()], b"aaa")
            .unwrap();
        assert_ne!(c1.from_head, c1.to_head);
        assert_eq!(c1.from_head, empty);
        let a_blob = c1.blob;
        let c2 = wp
            .write_file(lead, &["src".into(), "b.txt".into()], b"bbb")
            .unwrap();
        assert_eq!(c2.from_head, c1.to_head);
        assert_eq!(wp.get_blob(&a_blob).unwrap(), b"aaa");
        assert_eq!(
            wp.read_file(&["src".into(), "a.txt".into()]).unwrap(),
            Some(b"aaa".to_vec())
        );
        assert_eq!(
            wp.read_file(&["src".into(), "b.txt".into()]).unwrap(),
            Some(b"bbb".to_vec())
        );
        let err = wp
            .write_file(other, &["x".into()], b"no")
            .unwrap_err();
        assert!(matches!(err, WorkplaceError::NotLead));
    }

    #[test]
    fn branch_write_does_not_move_previous_head_snapshot() {
        let lead = Uuid::new_v4();
        let tmp = tmp();
        let wp = Workplace::create(tmp.path(), lead).unwrap();
        wp.write_file(lead, &["a".into()], b"1").unwrap();
        let main_head = wp.head().unwrap();
        wp.branch(lead, "priv").unwrap();
        wp.checkout(lead, "priv").unwrap();
        wp.write_file(lead, &["a".into()], b"2").unwrap();
        assert_ne!(wp.head().unwrap(), main_head);
        wp.checkout(lead, "priv").unwrap();
        wp.reset(lead, main_head).unwrap();
        assert_eq!(wp.head().unwrap(), main_head);
        assert_eq!(wp.read_file(&["a".into()]).unwrap(), Some(b"1".to_vec()));
    }

    #[test]
    fn workers_wp_round_trip_and_suffix_is_not_json() {
        let lead = Uuid::new_v4();
        let tmp = tmp();
        let wp = Workplace::create(tmp.path(), lead).unwrap();
        wp.set_workers(lead, b"alice\nbob\n").unwrap();
        assert_eq!(WORKERS_FILE.last().copied(), Some("workers.wp"));
        assert_eq!(wp.workers().unwrap(), Some(b"alice\nbob\n".to_vec()));
        assert!(wp
            .set_permit(Uuid::new_v4(), b"no")
            .is_err());
    }

    #[test]
    fn merge_ff_onto_main_and_non_lead_cannot_branch() {
        let lead = Uuid::new_v4();
        let tmp = tmp();
        let wp = Workplace::create(tmp.path(), lead).unwrap();
        wp.write_file(lead, &["a".into()], b"1").unwrap();
        wp.branch(lead, "main").unwrap();
        wp.checkout(lead, "main").unwrap();
        wp.branch(lead, "feat").unwrap();
        wp.checkout(lead, "feat").unwrap();
        wp.write_file(lead, &["a".into()], b"2").unwrap();
        let feat_head = wp.head().unwrap();
        wp.checkout(lead, "main").unwrap();
        assert_eq!(wp.read_file(&["a".into()]).unwrap(), Some(b"1".to_vec()));
        wp.merge_ff(lead, "feat").unwrap();
        assert_eq!(wp.head().unwrap(), feat_head);
        assert_eq!(wp.read_file(&["a".into()]).unwrap(), Some(b"2".to_vec()));
        assert!(matches!(
            wp.branch(Uuid::new_v4(), "x"),
            Err(WorkplaceError::NotLead)
        ));
    }
}
