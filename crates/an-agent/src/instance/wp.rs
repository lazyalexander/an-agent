//! Workplace owned by the steward. Live sub-workplaces hang on its tree.
//! A view is published only when a sub-workplace closes with writes.
//! Bytes stay in the worker session; this log only records versions.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::det_seam::Entropy;

#[derive(Debug, Error)]
pub enum WpError {
    #[error("sub-workplace is not registered: {0}")]
    NotRegistered(String),
    #[error("sub-workplace is waiting on a name conflict: {0}")]
    Pending(String),
    #[error("nothing to continue at {0}")]
    NoCurrent(String),
    #[error("continue of {true_name} is based on {base}, current is {current}")]
    StaleBase {
        true_name: String,
        current: u32,
        base: u32,
    },
    #[error("path escapes the sub-workplace: {0}")]
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

    /// Drop a live sub-workplace without publishing a view. Bytes stay put.
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

fn parse_uuid(s: &str) -> Result<Uuid, WpError> {
    Uuid::parse_str(s).map_err(|_| WpError::NotRegistered(s.into()))
}

fn next_view_id(views: &[ViewRec]) -> String {
    format!("v{}", views.len() + 1)
}

fn ensure_log(session: &Path) -> Result<(), WpError> {
    let dir = session.join("subwp");
    fs::create_dir_all(dir.join("blobs"))?;
    let log = dir.join("log.jsonl");
    if !log.exists() {
        fs::write(log, b"")?;
    }
    Ok(())
}

fn write_blob(session: &Path, bytes: &[u8]) -> Result<String, WpError> {
    let sha = format!("{:x}", Sha256::digest(bytes));
    let path = session.join("subwp/blobs").join(&sha);
    if !path.exists() {
        let tmp = session.join("subwp/blobs").join(format!("{sha}.tmp"));
        fs::write(&tmp, bytes)?;
        fs::rename(tmp, path)?;
    }
    Ok(sha)
}

fn append_sub_line(session: &Path, value: &Value) -> Result<(), WpError> {
    let mut file = OpenOptions::new()
        .append(true)
        .open(session.join("subwp/log.jsonl"))?;
    writeln!(file, "{value}")?;
    file.sync_all()?;
    Ok(())
}

fn read_log(session: &Path) -> Result<Vec<Value>, WpError> {
    let text = fs::read_to_string(session.join("subwp/log.jsonl"))?;
    let mut out = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        out.push(serde_json::from_str(line)?);
    }
    Ok(out)
}

fn log_has_change(session: &Path) -> Result<bool, WpError> {
    Ok(!read_log(session)?.is_empty())
}

fn inherited_name(views: &[ViewRec], session: &Path, path: &str) -> Result<String, WpError> {
    Ok(current_of(views, session, path)?.0)
}

fn current_of(views: &[ViewRec], session: &Path, path: &str) -> Result<(String, u32), WpError> {
    if let Some(found) = last_write(session, path)? {
        return Ok(found);
    }
    views
        .last()
        .and_then(|v| v.files.iter().find(|f| f.path == path))
        .map(|f| (f.true_name.clone(), f.version))
        .ok_or_else(|| WpError::NoCurrent(path.into()))
}

fn last_write(session: &Path, path: &str) -> Result<Option<(String, u32)>, WpError> {
    let mut found = None;
    for line in read_log(session)? {
        if line["path"].as_str() != Some(path) || line["op"].as_str() != Some("write") {
            continue;
        }
        if let (Some(name), Some(ver)) = (line["true_name"].as_str(), line["version"].as_u64()) {
            found = Some((name.to_string(), ver as u32));
        }
    }
    Ok(found)
}

fn next_version(views: &[ViewRec], session: &Path, true_name: &str) -> Result<u32, WpError> {
    let mut max = 0u32;
    for view in views {
        for file in &view.files {
            if file.true_name == true_name {
                max = max.max(file.version);
            }
        }
    }
    for line in read_log(session)? {
        if line["true_name"].as_str() == Some(true_name) {
            max = max.max(line["version"].as_u64().unwrap_or(0) as u32);
        }
    }
    Ok(max + 1)
}

fn reduce(
    current: Option<&ViewRec>,
    session: &Path,
) -> Result<(Vec<FileRec>, Option<Conflict>), WpError> {
    let mut files: BTreeMap<String, FileRec> = BTreeMap::new();
    if let Some(view) = current {
        for file in &view.files {
            files.insert(file.path.clone(), file.clone());
        }
    }
    let mut conflict = None;
    for line in read_log(session)? {
        let path = line["path"].as_str().unwrap_or("").to_string();
        match line["op"].as_str() {
            Some("tombstone") => {
                files.remove(&path);
            }
            Some("write") => {
                let rec = FileRec {
                    true_name: line["true_name"].as_str().unwrap_or("").to_string(),
                    version: line["version"].as_u64().unwrap_or(0) as u32,
                    path: path.clone(),
                    sha256: line["sha256"].as_str().unwrap_or("").to_string(),
                };
                if let Some(old) = files.get(&path)
                    && old.true_name != rec.true_name
                    && conflict.is_none()
                {
                    conflict = Some(Conflict {
                        path: path.clone(),
                        existing: old.true_name.clone(),
                        incoming: rec.true_name.clone(),
                    });
                }
                files.insert(path, rec);
            }
            _ => {}
        }
    }
    Ok((files.into_values().collect(), conflict))
}

fn chosen_rec(
    current: Option<&ViewRec>,
    session: &Path,
    true_name: &str,
    path: &str,
) -> Result<FileRec, WpError> {
    let mut found = None;
    for line in read_log(session)? {
        if line["op"].as_str() == Some("write") && line["true_name"].as_str() == Some(true_name) {
            found = Some(FileRec {
                true_name: true_name.to_string(),
                version: line["version"].as_u64().unwrap_or(0) as u32,
                path: path.to_string(),
                sha256: line["sha256"].as_str().unwrap_or("").to_string(),
            });
        }
    }
    if let Some(rec) = found {
        return Ok(rec);
    }
    current
        .and_then(|v| v.files.iter().find(|f| f.true_name == true_name).cloned())
        .map(|mut f| {
            f.path = path.to_string();
            f
        })
        .ok_or_else(|| WpError::MissingVersion {
            true_name: true_name.into(),
            version: 0,
        })
}

fn check_edges(files: &[FileRec], extra: &[DepEdge]) -> Result<(), WpError> {
    let have: BTreeSet<_> = files
        .iter()
        .map(|f| (f.true_name.clone(), f.version))
        .collect();
    for edge in extra {
        let dep = (&edge.dependent.true_name, edge.dependent.version);
        let on = (&edge.depends_on.true_name, edge.depends_on.version);
        if !have.contains(&(dep.0.clone(), dep.1)) || !have.contains(&(on.0.clone(), on.1)) {
            return Err(WpError::MissingDep);
        }
    }
    let mut graph: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for edge in extra {
        graph
            .entry(&edge.dependent.true_name)
            .or_default()
            .push(&edge.depends_on.true_name);
    }
    let mut color: BTreeMap<&str, u8> = BTreeMap::new();
    fn visit<'a>(
        n: &'a str,
        graph: &BTreeMap<&'a str, Vec<&'a str>>,
        color: &mut BTreeMap<&'a str, u8>,
    ) -> bool {
        match color.get(n).copied() {
            Some(1) => return true,
            Some(2) => return false,
            _ => {}
        }
        color.insert(n, 1);
        if let Some(next) = graph.get(n) {
            for m in next {
                if visit(m, graph, color) {
                    return true;
                }
            }
        }
        color.insert(n, 2);
        false
    }
    for n in graph.keys().copied() {
        if visit(n, &graph, &mut color) {
            return Err(WpError::Cycle);
        }
    }
    Ok(())
}

fn find_version(views: &[ViewRec], true_name: &str, version: u32) -> Result<FileRec, WpError> {
    if version == 0 {
        return Err(WpError::MissingVersion {
            true_name: true_name.into(),
            version: 0,
        });
    }
    for view in views.iter().rev() {
        if let Some(file) = view
            .files
            .iter()
            .find(|f| f.true_name == true_name && f.version == version)
        {
            return Ok(file.clone());
        }
    }
    Err(WpError::MissingVersion {
        true_name: true_name.into(),
        version,
    })
}

fn apply_version(files: &mut Vec<FileRec>, historical: &FileRec) {
    files.retain(|f| f.true_name != historical.true_name && f.path != historical.path);
    files.push(historical.clone());
}

fn clean_path(path: &str) -> Result<String, WpError> {
    if path.is_empty() || path.starts_with('/') || path.contains('\\') {
        return Err(WpError::BadPath(path.into()));
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(WpError::BadPath(path.into()));
        }
        parts.push(part);
    }
    Ok(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::act::{ActSentence, BareFile, FileFacet, Ingest, MemoryFacet, Permit, ToolTag};
    use crate::instance::Steward;
    use crate::principal::card::{AgentCard, ModelSpec, ToolGrant, Topology};
    use crate::testkit::TempDir;

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

    fn steward(tmp: &TempDir) -> Steward {
        Steward::open(
            &card("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", Permit::Deny),
            tmp.path(),
            "{}",
            "[]",
        )
        .unwrap()
    }

    fn bound() -> ActSentence {
        ActSentence::bare(Permit::Go, BareFile::None, Ingest::Ignore)
    }

    fn worker(s: &Steward) -> (std::sync::Arc<crate::instance::Agent>, String) {
        let w = s
            .spawn_worker(
                &card("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb", Permit::Go),
                &bound(),
            )
            .unwrap();
        let id = s.workplace().subwp_of(w.id()).unwrap();
        (w, id)
    }

    #[test]
    fn continue_replaces_same_true_name_and_keeps_bytes() {
        let tmp = TempDir::new("wp-continue");
        let s = steward(&tmp);
        let (w1, sub1) = worker(&s);
        let name = s
            .workplace()
            .write(&sub1, "src/a.txt", b"v1", WriteMode::Create)
            .unwrap();
        assert!(matches!(
            s.close_worker(w1.id(), &[]).unwrap(),
            CloseOut::Published(_)
        ));
        let blob = w1
            .session()
            .root()
            .join("subwp/blobs")
            .join(format!("{:x}", Sha256::digest(b"v1")));
        let (w2, sub2) = {
            let w = s
                .spawn_worker(
                    &card("cccccccc-cccc-cccc-cccc-cccccccccccc", Permit::Go),
                    &bound(),
                )
                .unwrap();
            let id = s.workplace().subwp_of(w.id()).unwrap();
            (w, id)
        };
        let again = s
            .workplace()
            .write(&sub2, "src/a.txt", b"v2", WriteMode::Continue { base: 1 })
            .unwrap();
        assert_eq!(again, name);
        s.close_worker(w2.id(), &[]).unwrap();
        let paths = s.workplace().current_paths();
        assert_eq!(paths, vec![("src/a.txt".into(), name, 2)]);
        assert_eq!(std::fs::read(blob).unwrap(), b"v1");
    }

    #[test]
    fn continue_rejects_a_stale_base_without_writing() {
        let tmp = TempDir::new("wp-stale");
        let s = steward(&tmp);
        let (w1, sub1) = worker(&s);
        s.workplace()
            .write(&sub1, "src/a.txt", b"v1", WriteMode::Create)
            .unwrap();
        s.close_worker(w1.id(), &[]).unwrap();
        let (w2, sub2) = {
            let w = s
                .spawn_worker(
                    &card("cccccccc-cccc-cccc-cccc-cccccccccccc", Permit::Go),
                    &bound(),
                )
                .unwrap();
            let id = s.workplace().subwp_of(w.id()).unwrap();
            (w, id)
        };
        let err = s
            .workplace()
            .write(&sub2, "src/a.txt", b"v2", WriteMode::Continue { base: 9 })
            .unwrap_err();
        assert!(matches!(
            err,
            WpError::StaleBase {
                current: 1,
                base: 9,
                ..
            }
        ));
        let log = std::fs::read_to_string(w2.session().root().join("subwp/log.jsonl")).unwrap();
        assert!(!log.contains("\"op\":\"write\"") && !log.contains("\"op\": \"write\""));
    }

    #[test]
    fn create_on_taken_path_asks_then_reject_or_rename() {
        let tmp = TempDir::new("wp-conflict");
        let s = steward(&tmp);
        let (w1, sub1) = worker(&s);
        s.workplace()
            .write(&sub1, "src/a.txt", b"old", WriteMode::Create)
            .unwrap();
        s.close_worker(w1.id(), &[]).unwrap();
        let (w2, sub2) = {
            let w = s
                .spawn_worker(
                    &card("cccccccc-cccc-cccc-cccc-cccccccccccc", Permit::Go),
                    &bound(),
                )
                .unwrap();
            let id = s.workplace().subwp_of(w.id()).unwrap();
            (w, id)
        };
        s.workplace()
            .write(&sub2, "src/a.txt", b"new", WriteMode::Create)
            .unwrap();
        let conflict = s.close_worker(w2.id(), &[]).unwrap();
        let CloseOut::Conflict(c) = conflict else {
            panic!("expected conflict");
        };
        assert!(s.workplace().is_live(&sub2));
        s.workplace()
            .resolve(
                &sub2,
                Resolve::Allow {
                    true_name: c.incoming.clone(),
                    path: "src/b.txt".into(),
                },
            )
            .unwrap();
        assert!(!s.workplace().is_live(&sub2));
        let paths = s.workplace().current_paths();
        assert!(paths.iter().any(|p| p.0 == "src/a.txt"));
        assert!(
            paths
                .iter()
                .any(|p| p.0 == "src/b.txt" && p.1 == c.incoming)
        );
    }

    #[test]
    fn empty_close_publishes_nothing() {
        let tmp = TempDir::new("wp-empty");
        let s = steward(&tmp);
        let (w, sub) = worker(&s);
        assert!(matches!(
            s.close_worker(w.id(), &[]).unwrap(),
            CloseOut::Empty
        ));
        assert!(!s.workplace().is_live(&sub));
        assert!(s.workplace().current_paths().is_empty());
    }

    #[test]
    fn cyclic_extra_is_rejected() {
        let tmp = TempDir::new("wp-cycle");
        let s = steward(&tmp);
        let (w, sub) = worker(&s);
        let a = s
            .workplace()
            .write(&sub, "src/a.txt", b"a", WriteMode::Create)
            .unwrap();
        let b = s
            .workplace()
            .write(&sub, "src/b.txt", b"b", WriteMode::Create)
            .unwrap();
        let err = s.close_worker(
            w.id(),
            &[
                DepEdge {
                    dependent: VerId {
                        true_name: a.clone(),
                        version: 1,
                    },
                    depends_on: VerId {
                        true_name: b.clone(),
                        version: 1,
                    },
                },
                DepEdge {
                    dependent: VerId {
                        true_name: b,
                        version: 1,
                    },
                    depends_on: VerId {
                        true_name: a,
                        version: 1,
                    },
                },
            ],
        );
        assert!(matches!(
            err,
            Err(crate::instance::StewardError::Wp(WpError::Cycle))
        ));
        assert!(s.workplace().is_live(&sub));
    }

    #[test]
    fn rollback_without_cascade_leaves_dependents() {
        let tmp = TempDir::new("wp-roll-quiet");
        let s = steward(&tmp);
        let (w, sub) = worker(&s);
        let a = s
            .workplace()
            .write(&sub, "src/a.txt", b"a1", WriteMode::Create)
            .unwrap();
        s.close_worker(w.id(), &[]).unwrap();
        let (w2, sub2) = {
            let w = s
                .spawn_worker(
                    &card("cccccccc-cccc-cccc-cccc-cccccccccccc", Permit::Go),
                    &bound(),
                )
                .unwrap();
            let id = s.workplace().subwp_of(w.id()).unwrap();
            (w, id)
        };
        s.workplace()
            .write(&sub2, "src/a.txt", b"a2", WriteMode::Continue { base: 1 })
            .unwrap();
        let b = s
            .workplace()
            .write(&sub2, "src/b.txt", b"b1", WriteMode::Create)
            .unwrap();
        s.close_worker(
            w2.id(),
            &[DepEdge {
                dependent: VerId {
                    true_name: b.clone(),
                    version: 1,
                },
                depends_on: VerId {
                    true_name: a.clone(),
                    version: 2,
                },
            }],
        )
        .unwrap();
        s.workplace().rollback(&a, 1, false).unwrap();
        let quiet = s.workplace().current_paths();
        assert!(quiet.iter().any(|p| p.1 == a && p.2 == 1));
        assert!(quiet.iter().any(|p| p.1 == b && p.2 == 1));
    }

    #[test]
    fn rollback_can_cascade_and_keeps_prior_view() {
        let tmp = TempDir::new("wp-roll");
        let s = steward(&tmp);
        let (w, sub) = worker(&s);
        let a = s
            .workplace()
            .write(&sub, "src/a.txt", b"a1", WriteMode::Create)
            .unwrap();
        s.close_worker(w.id(), &[]).unwrap();
        let (w2, sub2) = {
            let w = s
                .spawn_worker(
                    &card("cccccccc-cccc-cccc-cccc-cccccccccccc", Permit::Go),
                    &bound(),
                )
                .unwrap();
            let id = s.workplace().subwp_of(w.id()).unwrap();
            (w, id)
        };
        s.workplace()
            .write(&sub2, "src/a.txt", b"a2", WriteMode::Continue { base: 1 })
            .unwrap();
        let b = s
            .workplace()
            .write(&sub2, "src/b.txt", b"b1", WriteMode::Create)
            .unwrap();
        s.close_worker(
            w2.id(),
            &[DepEdge {
                dependent: VerId {
                    true_name: b.clone(),
                    version: 1,
                },
                depends_on: VerId {
                    true_name: a.clone(),
                    version: 2,
                },
            }],
        )
        .unwrap();
        let before = std::fs::read(tmp.path().join("wp/views.jsonl")).unwrap();
        s.workplace().rollback(&a, 1, true).unwrap();
        let cascaded = s.workplace().current_paths();
        assert!(cascaded.iter().any(|p| p.1 == a && p.2 == 1));
        assert!(
            !cascaded.iter().any(|p| p.1 == b),
            "dependent introduced with A@2 leaves the view"
        );
        let after = std::fs::read(tmp.path().join("wp/views.jsonl")).unwrap();
        assert!(after.starts_with(&before));
    }

    #[test]
    fn unclosed_subwp_is_not_the_view() {
        let tmp = TempDir::new("wp-open");
        let s = steward(&tmp);
        let (_w, sub) = worker(&s);
        s.workplace()
            .write(&sub, "src/a.txt", b"x", WriteMode::Create)
            .unwrap();
        assert!(s.workplace().is_live(&sub));
        assert!(s.workplace().current_paths().is_empty());
    }
}
