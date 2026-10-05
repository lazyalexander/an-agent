//! Sidecar index and markdown segments. The index can be deleted and rebuilt.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use an_agent_core::memstream::Memevent;

use super::ContextError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Row {
    pub(super) event_id: String,
    pub(super) weak: u64,
    pub(super) segment: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct Segment {
    pub(super) id: String,
    pub(super) text: String,
    pub(super) sources: Vec<(String, f64)>,
}

pub fn rebuild_index(session_dir: &Path, events: &[Memevent]) -> Result<(), ContextError> {
    let segments = load_segments(session_dir)?;
    let mut cited: BTreeMap<String, String> = BTreeMap::new();
    for seg in &segments {
        for (id, _) in &seg.sources {
            cited.insert(id.clone(), seg.id.clone());
        }
    }
    let rows: Vec<Row> = events
        .iter()
        .map(|e| Row {
            event_id: e.id.clone(),
            weak: 0,
            segment: cited.get(&e.id).cloned(),
        })
        .collect();
    save_rows(session_dir, &rows)
}

pub fn segment_sources(
    session_dir: &Path,
    md_file: &Path,
) -> Result<Vec<(String, f64)>, ContextError> {
    let text = fs::read_to_string(md_file)?;
    let mut out = Vec::new();
    for seg in parse_md(
        &text,
        md_file.file_name().and_then(|s| s.to_str()).unwrap_or("md"),
    )? {
        let sum: f64 = seg.sources.iter().map(|(_, c)| c).sum();
        if (sum - 1.0).abs() > 1e-6 && !seg.sources.is_empty() {
            return Err(ContextError::BadMd);
        }
        out.extend(seg.sources);
    }
    let _ = session_dir;
    Ok(out)
}

pub(super) fn sync_rows(events: &[Memevent], rows: Vec<Row>) -> Vec<Row> {
    let mut by_id: BTreeMap<String, Row> =
        rows.into_iter().map(|r| (r.event_id.clone(), r)).collect();
    for event in events {
        by_id.entry(event.id.clone()).or_insert(Row {
            event_id: event.id.clone(),
            weak: 0,
            segment: None,
        });
    }
    let mut out: Vec<Row> = by_id.into_values().collect();
    out.sort_by(|a, b| a.event_id.cmp(&b.event_id));
    out
}

pub(super) fn weak_of(rows: &[Row], sources: &[(String, f64)]) -> u64 {
    sources
        .iter()
        .map(|(id, _)| {
            rows.iter()
                .find(|r| r.event_id == *id)
                .map(|r| r.weak)
                .unwrap_or(0)
        })
        .max()
        .unwrap_or(0)
}

pub(super) fn md_dir(session_dir: &Path) -> PathBuf {
    session_dir.join("ctx/md")
}

pub(super) fn index_path(session_dir: &Path) -> PathBuf {
    session_dir.join("ctx/index.jsonl")
}

pub(super) fn load_rows(session_dir: &Path) -> Result<Vec<Row>, ContextError> {
    let path = index_path(session_dir);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut rows = Vec::new();
    for line in fs::read_to_string(path)?.lines() {
        if line.is_empty() {
            continue;
        }
        rows.push(serde_json::from_str(line)?);
    }
    Ok(rows)
}

pub(super) fn save_rows(session_dir: &Path, rows: &[Row]) -> Result<(), ContextError> {
    fs::create_dir_all(session_dir.join("ctx"))?;
    let path = index_path(session_dir);
    let tmp = session_dir.join("ctx/index.jsonl.tmp");
    {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp)?;
        for row in rows {
            writeln!(file, "{}", serde_json::to_string(row)?)?;
        }
    }
    fs::rename(tmp, path)?;
    Ok(())
}

pub(super) fn load_segments(session_dir: &Path) -> Result<Vec<Segment>, ContextError> {
    let dir = md_dir(session_dir);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut names: Vec<_> = fs::read_dir(&dir)?.filter_map(|e| e.ok()).collect();
    names.sort_by_key(|e| e.file_name());
    let mut out = Vec::new();
    for entry in names {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.ends_with(".md") {
            continue;
        }
        let text = fs::read_to_string(entry.path())?;
        out.extend(parse_md(&text, &name)?);
    }
    Ok(out)
}

fn parse_md(text: &str, file: &str) -> Result<Vec<Segment>, ContextError> {
    let mut out = Vec::new();
    let mut current: Option<(String, String)> = None;
    let mut body = String::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            if let Some((id, sources)) = current.take() {
                out.push(Segment {
                    id,
                    text: body.trim().to_string(),
                    sources: parse_sources(&sources)?,
                });
                body.clear();
            }
            current = Some((format!("{file}#{rest}"), String::new()));
        } else if let Some(inner) = line
            .strip_prefix("<!-- ")
            .and_then(|s| s.strip_suffix(" -->"))
        {
            if let Some((_, sources)) = current.as_mut() {
                *sources = inner.to_string();
            }
        } else if current.is_some() {
            body.push_str(line);
            body.push('\n');
        }
    }
    if let Some((id, sources)) = current.take() {
        out.push(Segment {
            id,
            text: body.trim().to_string(),
            sources: parse_sources(&sources)?,
        });
    }
    Ok(out)
}

fn parse_sources(raw: &str) -> Result<Vec<(String, f64)>, ContextError> {
    if raw.is_empty() {
        return Err(ContextError::BadMd);
    }
    let v: serde_json::Value = serde_json::from_str(raw)?;
    let arr = v.as_array().ok_or(ContextError::BadMd)?;
    let mut out = Vec::new();
    for item in arr {
        let id = item["event_id"]
            .as_str()
            .ok_or(ContextError::BadMd)?
            .to_string();
        let contribution = item["contribution"].as_f64().ok_or(ContextError::BadMd)?;
        out.push((id, contribution));
    }
    Ok(out)
}
