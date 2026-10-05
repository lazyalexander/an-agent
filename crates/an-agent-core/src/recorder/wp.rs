//! Workspace owned by the recorder. Live sub-workspaces hang on its tree.
//! A view is published only when a sub-workspace closes with writes.
//! Bytes stay in the worker session; this log only records versions.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use uuid::Uuid;

use crate::det_seam::Entropy;

mod log;

use log::{
    append_sub_line, apply_version, check_edges, chosen_rec, clean_path, current_of, ensure_log,
    find_version, inherited_name, log_has_change, next_version, next_view_id, parse_uuid, reduce,
    write_blob,
};

#[derive(Debug, Error)]
pub enum WpError {
    #[error("sub-workspace is not registered: {0}")]
    NotRegistered(String),
    #[error("sub-workspace is waiting on a name conflict: {0}")]
    Pending(String),
    #[error("nothing to continue at {0}")]
    NoCurrent(String),
    #[error("continue of {true_name} is based on {base}, current is {current}")]
    StaleBase {
        true_name: String,
        current: u32,
        base: u32,
    },
    #[error("path escapes the sub-workspace: {0}")]
    BadPath(String),
    #[error("dependency cycle")]
    Cycle,
    #[error("dependency names a missing version")]
    MissingDep,
    #[error("no published view")]
    NoView,
    #[error("version not in history: {true_name}@{version}")]
    MissingVersion { true_name: String, version: u32 },
    #[error("conflict choice is neither name")]
    BadChoice,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    /// Reuse the true name. `base` must be the current version of that name.
    Continue { base: u32 },
    /// Mint a new true name even if the path is already taken.
    Create,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerId {
    pub true_name: String,
    pub version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepEdge {
    pub dependent: VerId,
    pub depends_on: VerId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub path: String,
    pub existing: String,
    pub incoming: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseOut {
    Empty,
    Published(String),
    Conflict(Conflict),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolve {
    /// This true name occupies `path` in the new view. The other name does not.
    Allow {
        true_name: String,
        path: String,
    },
    Reject,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FileRec {
    true_name: String,
    version: u32,
    path: String,
    sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ViewRec {
    id: String,
    files: Vec<FileRec>,
    edges: Vec<DepEdge>,
}

#[derive(Clone)]
struct Live {
    worker: Uuid,
    session: PathBuf,
    pending: Option<Conflict>,
}

pub struct Wp {
    root: PathBuf,
    lock: Mutex<WpState>,
}

struct WpState {
    live: BTreeMap<String, Live>,
    views: Vec<ViewRec>,
}

impl Wp {
    pub fn open(sessions_root: &Path) -> Result<Self, WpError> {
        let root = sessions_root.join("wp");
        fs::create_dir_all(&root)?;
        let views_path = root.join("views.jsonl");
        if !views_path.exists() {
            fs::write(&views_path, b"")?;
        }
        let tree_path = root.join("tree.jsonl");
        if !tree_path.exists() {
            fs::write(&tree_path, b"")?;
        }
        let mut state = WpState {
            live: BTreeMap::new(),
            views: Vec::new(),
        };
        for line in fs::read_to_string(&views_path)?.lines() {
            if line.is_empty() {
                continue;
            }
            state.views.push(serde_json::from_str(line)?);
        }
        for line in fs::read_to_string(&tree_path)?.lines() {
            if line.is_empty() {
                continue;
            }
            let v: Value = serde_json::from_str(line)?;
            match v["op"].as_str() {
                Some("mount") => {
                    let id = v["id"].as_str().unwrap_or_default().to_string();
                    state.live.insert(
                        id,
                        Live {
                            worker: parse_uuid(v["worker"].as_str().unwrap_or_default())?,
                            session: PathBuf::from(v["session"].as_str().unwrap_or_default()),
                            pending: None,
                        },
                    );
                }
                Some("unmount") => {
                    if let Some(id) = v["id"].as_str() {
                        state.live.remove(id);
                    }
                }
                _ => {}
            }
        }
        Ok(Self {
            root,
            lock: Mutex::new(state),
        })
    }

    pub fn register(&self, worker: Uuid, session: &Path) -> Result<String, WpError> {
        let mut state = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        if state.live.values().any(|l| l.worker == worker) {
            return Err(WpError::Pending(worker.to_string()));
        }
        let id = Entropy::os().uuid_v4().to_string();
        ensure_log(session)?;
        self.append_tree(&json!({
            "op": "mount",
            "id": id,
            "worker": worker.to_string(),
            "session": session,
        }))?;
        state.live.insert(
            id.clone(),
            Live {
                worker,
                session: session.to_path_buf(),
                pending: None,
            },
        );
        Ok(id)
    }

    /// Drop a live sub-workspace without publishing a view. Bytes stay put.
    pub fn abandon_worker(&self, worker: Uuid) -> Result<bool, WpError> {
        let mut state = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let Some(sub) = state
            .live
            .iter()
            .find(|(_, live)| live.worker == worker)
            .map(|(id, _)| id.clone())
        else {
            return Ok(false);
        };
        self.unmount_locked(&mut state, &sub)?;
        Ok(true)
    }

    pub fn subwp_of(&self, worker: Uuid) -> Option<String> {
        self.lock
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .live
            .iter()
            .find(|(_, l)| l.worker == worker)
            .map(|(id, _)| id.clone())
    }

    pub fn write(
        &self,
        sub: &str,
        path: &str,
        bytes: &[u8],
        mode: WriteMode,
    ) -> Result<String, WpError> {
        let path = clean_path(path)?;
        let state = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let live = state
            .live
            .get(sub)
            .ok_or_else(|| WpError::NotRegistered(sub.into()))?
            .clone();
        if live.pending.is_some() {
            return Err(WpError::Pending(sub.into()));
        }
        let true_name = match mode {
            WriteMode::Continue { base } => {
                let (name, current) = current_of(&state.views, &live.session, &path)?;
                if current != base {
                    return Err(WpError::StaleBase {
                        true_name: name,
                        current,
                        base,
                    });
                }
                name
            }
            WriteMode::Create => Entropy::os().uuid_v4().to_string(),
        };
        let version = next_version(&state.views, &live.session, &true_name)?;
        let sha = write_blob(&live.session, bytes)?;
        append_sub_line(
            &live.session,
            &json!({
                "op": "write",
                "path": path,
                "sha256": sha,
                "true_name": true_name,
                "version": version,
                "base": match mode {
                    WriteMode::Continue { base } => Some(base),
                    WriteMode::Create => None,
                },
            }),
        )?;
        Ok(true_name)
    }

    pub fn tombstone(&self, sub: &str, path: &str) -> Result<(), WpError> {
        let path = clean_path(path)?;
        let state = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let live = state
            .live
            .get(sub)
            .ok_or_else(|| WpError::NotRegistered(sub.into()))?
            .clone();
        if live.pending.is_some() {
            return Err(WpError::Pending(sub.into()));
        }
        let true_name = inherited_name(&state.views, &live.session, &path)?;
        append_sub_line(
            &live.session,
            &json!({ "op": "tombstone", "path": path, "true_name": true_name }),
        )
    }

    pub fn close(&self, sub: &str, extra: &[DepEdge]) -> Result<CloseOut, WpError> {
        let mut state = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let live = state
            .live
            .get(sub)
            .ok_or_else(|| WpError::NotRegistered(sub.into()))?
            .clone();
        if live.pending.is_some() {
            return Err(WpError::Pending(sub.into()));
        }
        if !log_has_change(&live.session)? {
            self.unmount_locked(&mut state, sub)?;
            return Ok(CloseOut::Empty);
        }
        let current = state.views.last().cloned();
        let (files, conflict) = reduce(current.as_ref(), &live.session)?;
        if let Some(conflict) = conflict {
            if let Some(slot) = state.live.get_mut(sub) {
                slot.pending = Some(conflict.clone());
            }
            return Ok(CloseOut::Conflict(conflict));
        }
        check_edges(&files, extra)?;
        let id = self.publish_locked(&mut state, sub, files, extra.to_vec())?;
        Ok(CloseOut::Published(id))
    }

    pub fn resolve(&self, sub: &str, choice: Resolve) -> Result<CloseOut, WpError> {
        let mut state = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let live = state
            .live
            .get(sub)
            .ok_or_else(|| WpError::NotRegistered(sub.into()))?
            .clone();
        let Some(conflict) = live.pending.clone() else {
            return Err(WpError::NoCurrent(sub.into()));
        };
        match choice {
            Resolve::Reject => {
                self.unmount_locked(&mut state, sub)?;
                Ok(CloseOut::Empty)
            }
            Resolve::Allow { true_name, path } => {
                if true_name != conflict.existing && true_name != conflict.incoming {
                    return Err(WpError::BadChoice);
                }
                let path = clean_path(&path)?;
                let current = state.views.last().cloned();
                let mut files = current
                    .as_ref()
                    .map(|v| v.files.clone())
                    .unwrap_or_default();
                files.retain(|f| f.true_name != true_name && f.path != path);
                let chosen = chosen_rec(current.as_ref(), &live.session, &true_name, &path)?;
                files.push(chosen);
                files.sort_by(|a, b| a.path.cmp(&b.path));
                let id = self.publish_locked(&mut state, sub, files, Vec::new())?;
                Ok(CloseOut::Published(id))
            }
        }
    }

    pub fn rollback(
        &self,
        true_name: &str,
        to_version: u32,
        cascade: bool,
    ) -> Result<String, WpError> {
        let mut state = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let current = state.views.last().cloned().ok_or(WpError::NoView)?;
        let historical = find_version(&state.views, true_name, to_version)?;
        let mut next: Vec<FileRec> = current.files.clone();
        apply_version(&mut next, &historical);
        if cascade {
            let leaving = current
                .files
                .iter()
                .find(|f| f.true_name == true_name)
                .map(|f| f.version)
                .ok_or(WpError::NoView)?;
            let mut stack = vec![(true_name.to_string(), leaving)];
            let mut seen = BTreeSet::from([true_name.to_string()]);
            while let Some((name, ver)) = stack.pop() {
                for edge in &current.edges {
                    if edge.depends_on.true_name != name || edge.depends_on.version != ver {
                        continue;
                    }
                    let dep = &edge.dependent.true_name;
                    if !seen.insert(dep.clone()) {
                        return Err(WpError::Cycle);
                    }
                    if edge.dependent.version <= 1 {
                        next.retain(|f| f.true_name != *dep);
                    } else {
                        let prev = find_version(&state.views, dep, edge.dependent.version - 1)?;
                        apply_version(&mut next, &prev);
                    }
                    stack.push((dep.clone(), edge.dependent.version));
                }
            }
        }
        next.sort_by(|a, b| a.path.cmp(&b.path));
        let id = next_view_id(&state.views);
        let rec = ViewRec {
            id: id.clone(),
            files: next,
            edges: current.edges.clone(),
        };
        self.append_view(&rec)?;
        state.views.push(rec);
        Ok(id)
    }

    pub fn current_paths(&self) -> Vec<(String, String, u32)> {
        let state = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        state
            .views
            .last()
            .map(|v| {
                v.files
                    .iter()
                    .map(|f| (f.path.clone(), f.true_name.clone(), f.version))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn is_live(&self, sub: &str) -> bool {
        self.lock
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .live
            .contains_key(sub)
    }

    fn publish_locked(
        &self,
        state: &mut WpState,
        sub: &str,
        mut files: Vec<FileRec>,
        edges: Vec<DepEdge>,
    ) -> Result<String, WpError> {
        files.sort_by(|a, b| a.path.cmp(&b.path));
        let id = next_view_id(&state.views);
        let rec = ViewRec {
            id: id.clone(),
            files,
            edges,
        };
        self.append_view(&rec)?;
        state.views.push(rec);
        self.unmount_locked(state, sub)?;
        Ok(id)
    }

    fn unmount_locked(&self, state: &mut WpState, sub: &str) -> Result<(), WpError> {
        state.live.remove(sub);
        self.append_tree(&json!({ "op": "unmount", "id": sub }))
    }

    fn append_view(&self, rec: &ViewRec) -> Result<(), WpError> {
        let mut file = OpenOptions::new()
            .append(true)
            .open(self.root.join("views.jsonl"))?;
        writeln!(file, "{}", serde_json::to_string(rec)?)?;
        file.sync_all()?;
        Ok(())
    }

    fn append_tree(&self, value: &Value) -> Result<(), WpError> {
        let mut file = OpenOptions::new()
            .append(true)
            .open(self.root.join("tree.jsonl"))?;
        writeln!(file, "{value}")?;
        file.sync_all()?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "wp/tests.rs"]
mod tests;
