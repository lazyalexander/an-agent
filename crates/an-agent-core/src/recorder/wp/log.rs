//! Version log for one sub-workspace: blobs, names, and the published view.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{Conflict, DepEdge, FileRec, ViewRec, WpError};

pub(super) fn parse_uuid(s: &str) -> Result<Uuid, WpError> {
    Uuid::parse_str(s).map_err(|_| WpError::NotRegistered(s.into()))
}

pub(super) fn next_view_id(views: &[ViewRec]) -> String {
    format!("v{}", views.len() + 1)
}

pub(super) fn ensure_log(session: &Path) -> Result<(), WpError> {
    let dir = session.join("subwp");
    fs::create_dir_all(dir.join("blobs"))?;
    let log = dir.join("log.jsonl");
    if !log.exists() {
        fs::write(log, b"")?;
    }
    Ok(())
}

pub(super) fn write_blob(session: &Path, bytes: &[u8]) -> Result<String, WpError> {
    let sha = format!("{:x}", Sha256::digest(bytes));
    let path = session.join("subwp/blobs").join(&sha);
    if !path.exists() {
        let tmp = session.join("subwp/blobs").join(format!("{sha}.tmp"));
        fs::write(&tmp, bytes)?;
        fs::rename(tmp, path)?;
    }
    Ok(sha)
}

pub(super) fn append_sub_line(session: &Path, value: &Value) -> Result<(), WpError> {
    let mut file = OpenOptions::new()
        .append(true)
        .open(session.join("subwp/log.jsonl"))?;
    writeln!(file, "{value}")?;
    file.sync_all()?;
    Ok(())
}

pub(super) fn read_log(session: &Path) -> Result<Vec<Value>, WpError> {
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

pub(super) fn log_has_change(session: &Path) -> Result<bool, WpError> {
    Ok(!read_log(session)?.is_empty())
}

pub(super) fn inherited_name(
    views: &[ViewRec],
    session: &Path,
    path: &str,
) -> Result<String, WpError> {
    Ok(current_of(views, session, path)?.0)
}

pub(super) fn current_of(
    views: &[ViewRec],
    session: &Path,
    path: &str,
) -> Result<(String, u32), WpError> {
    if let Some(found) = last_write(session, path)? {
        return Ok(found);
    }
    views
        .last()
        .and_then(|v| v.files.iter().find(|f| f.path == path))
        .map(|f| (f.true_name.clone(), f.version))
        .ok_or_else(|| WpError::NoCurrent(path.into()))
}

pub(super) fn last_write(session: &Path, path: &str) -> Result<Option<(String, u32)>, WpError> {
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

pub(super) fn next_version(
    views: &[ViewRec],
    session: &Path,
    true_name: &str,
) -> Result<u32, WpError> {
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

pub(super) fn reduce(
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

pub(super) fn chosen_rec(
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

pub(super) fn check_edges(files: &[FileRec], extra: &[DepEdge]) -> Result<(), WpError> {
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

pub(super) fn find_version(
    views: &[ViewRec],
    true_name: &str,
    version: u32,
) -> Result<FileRec, WpError> {
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

pub(super) fn apply_version(files: &mut Vec<FileRec>, historical: &FileRec) {
    files.retain(|f| f.true_name != historical.true_name && f.path != historical.path);
    files.push(historical.clone());
}

pub(super) fn clean_path(path: &str) -> Result<String, WpError> {
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
