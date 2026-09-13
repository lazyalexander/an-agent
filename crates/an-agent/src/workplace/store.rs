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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Meta {
    lead: Uuid,
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
        let id = Uuid::new_v4();
        let root = dir.as_ref().join(id.to_string());
        fs::create_dir_all(root.join("objects"))?;
        let wp = Self {
            id,
            root: root.clone(),
            lead,
        };
        fs::write(
            root.join("meta.json"),
            serde_json::to_vec_pretty(&Meta { lead })?,
        )?;
        let empty = wp.put_tree(&Tree::default())?;
        fs::write(root.join("HEAD"), format!("{empty}\n"))?;
        Ok(wp)
    }

    pub fn open(root: impl AsRef<Path>, id: Uuid) -> Result<Self, WorkplaceError> {
        let root = root.as_ref().join(id.to_string());
        let meta: Meta = serde_json::from_slice(&fs::read(root.join("meta.json"))?)?;
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
        fs::write(self.root.join("HEAD"), format!("{to_head}\n"))?;
        Ok(Commit {
            from_head,
            to_head,
            blob,
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use ulid::Ulid;

    fn tmp() -> PathBuf {
        std::env::temp_dir().join(format!("an-agent-wp-{}", Ulid::new()))
    }

    #[test]
    fn write_advances_head_and_shares_blobs() {
        let lead = Uuid::new_v4();
        let other = Uuid::new_v4();
        let wp = Workplace::create(tmp(), lead).unwrap();
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
}
