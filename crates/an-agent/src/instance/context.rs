//! Two-layer context for one session. The tape stays the record. Compressed
//! markdown cites event ids. The sidecar index can be deleted and rebuilt.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::memstream::{AppendEvent, FromKind, JsonlStore, Kind, Memevent};

#[derive(Debug, Error)]
pub enum ContextError {
    #[error("store: {0}")]
    Store(#[from] crate::memstream::StoreError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("session has no events")]
    Empty,
    #[error("md missing sources")]
    BadMd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssembleMode {
    Continue,
    Independent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    pub kind: &'static str,
    pub event_id: Option<String>,
    pub segment: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assembly {
    pub prompt: String,
    pub config: String,
    pub context: Vec<Piece>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Row {
    event_id: String,
    weak: u64,
    segment: Option<String>,
}

#[derive(Debug, Clone)]
struct Segment {
    id: String,
    text: String,
    sources: Vec<(String, f64)>,
}

/// Record a user summary and freeze everything since the previous cut into a
/// new md file. Earlier md files are not rewritten.
pub fn record_summary(session_dir: &Path, text: &str) -> Result<PathBuf, ContextError> {
    let tape = JsonlStore::open(session_dir.join("memory.jsonl"))?;
    let prior = tape.read_all()?;
    let anchor = prior.last().ok_or(ContextError::Empty)?;
    tape.append(AppendEvent {
        from: anchor.from.clone(),
        from_kind: anchor.from_kind,
        kind: Kind::Utterance,
        session: anchor.session.clone().unwrap_or_default(),
        content: text.to_string(),
        tags: vec!["summary".into()],
        refs: vec![],
        act: None,
        card: anchor.card.clone(),
    })?;
    cut_since_last(session_dir)
}

/// Freeze the uncompressed tail when its text is at least `max_chars`.
pub fn cut_if_long(session_dir: &Path, max_chars: usize) -> Result<Option<PathBuf>, ContextError> {
    let tape = JsonlStore::open(session_dir.join("memory.jsonl"))?;
    let events = tape.read_all()?;
    let span = uncompressed(&events);
    let n: usize = span.iter().map(|e| e.content.len()).sum();
    if n < max_chars || span.is_empty() {
        return Ok(None);
    }
    Ok(Some(write_cut(session_dir, &span)?))
}

pub fn assemble(
    session_dir: &Path,
    mode: AssembleMode,
    budget_chars: usize,
) -> Result<Assembly, ContextError> {
    let prompt = read_proj(session_dir, "prompt");
    let config = read_proj(session_dir, "config");
    if mode == AssembleMode::Independent {
        return Ok(Assembly {
            prompt,
            config,
            context: Vec::new(),
        });
    }
    if !index_path(session_dir).exists() {
        rebuild_index(session_dir)?;
    }
    let tape = JsonlStore::open(session_dir.join("memory.jsonl"))?;
    let events = tape.read_all()?;
    let mut rows = sync_rows(&events, load_rows(session_dir)?);
    let segments = load_segments(session_dir)?;
    let recent: Vec<&Memevent> = uncompressed(&events);
    let mut pieces = Vec::new();
    let mut used = 0usize;
    let mut selected_events: BTreeSet<String> = BTreeSet::new();
    let mut selected_segs: BTreeSet<String> = BTreeSet::new();

    for event in &recent {
        if !push_piece(
            &mut pieces,
            &mut used,
            budget_chars,
            Piece {
                kind: "recent",
                event_id: Some(event.id.clone()),
                segment: None,
                text: event.content.clone(),
            },
        ) {
            break;
        }
        selected_events.insert(event.id.clone());
    }

    let mut hot: Vec<&Segment> = segments.iter().collect();
    hot.sort_by(|a, b| {
        let wa = weak_of(&rows, &a.sources);
        let wb = weak_of(&rows, &b.sources);
        wb.cmp(&wa).then(b.id.cmp(&a.id))
    });
    if let Some(seg) = hot.first()
        && push_piece(
            &mut pieces,
            &mut used,
            budget_chars,
            Piece {
                kind: "hot",
                event_id: None,
                segment: Some(seg.id.clone()),
                text: seg.text.clone(),
            },
        )
    {
        selected_segs.insert(seg.id.clone());
        for (id, _) in &seg.sources {
            selected_events.insert(id.clone());
        }
    }

    let recent_ids: BTreeSet<_> = recent.iter().map(|e| e.id.as_str()).collect();
    for seg in &segments {
        if selected_segs.contains(&seg.id) {
            continue;
        }
        if !seg
            .sources
            .iter()
            .any(|(id, _)| recent_ids.contains(id.as_str()))
        {
            continue;
        }
        if !push_piece(
            &mut pieces,
            &mut used,
            budget_chars,
            Piece {
                kind: "summary",
                event_id: None,
                segment: Some(seg.id.clone()),
                text: seg.text.clone(),
            },
        ) {
            break;
        }
        for (id, _) in &seg.sources {
            selected_events.insert(id.clone());
        }
    }

    for row in &mut rows {
        if selected_events.contains(&row.event_id) {
            row.weak += 1;
        }
    }
    save_rows(session_dir, &rows)?;
    Ok(Assembly {
        prompt,
        config,
        context: pieces,
    })
}

pub fn rebuild_index(session_dir: &Path) -> Result<(), ContextError> {
    let tape = JsonlStore::open(session_dir.join("memory.jsonl"))?;
    let events = tape.read_all()?;
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

fn cut_since_last(session_dir: &Path) -> Result<PathBuf, ContextError> {
    let tape = JsonlStore::open(session_dir.join("memory.jsonl"))?;
    let events = tape.read_all()?;
    let span = uncompressed(&events);
    write_cut(session_dir, &span)
}

fn write_cut(session_dir: &Path, span: &[&Memevent]) -> Result<PathBuf, ContextError> {
    if span.is_empty() {
        return Err(ContextError::Empty);
    }
    let anchor = span[span.len() - 1];
    let mut body = String::from("# cut\n\n");
    let mut ids = Vec::new();
    for (i, event) in span.iter().enumerate() {
        ids.push(event.id.clone());
        let sources = json!([{ "event_id": event.id, "contribution": 1.0 }]);
        body.push_str(&format!(
            "## s{i}\n<!-- {sources} -->\n{}\n\n",
            event.content
        ));
    }
    let bytes = body.into_bytes();
    let sha = format!("{:x}", Sha256::digest(&bytes));
    let name = format!("{sha}.md");
    let path = md_dir(session_dir).join(&name);
    fs::create_dir_all(md_dir(session_dir))?;
    if !path.exists() {
        fs::write(&path, &bytes)?;
    }
    let tape = JsonlStore::open(session_dir.join("memory.jsonl"))?;
    tape.append(AppendEvent {
        from: anchor.from.clone(),
        from_kind: FromKind::Agent,
        kind: Kind::Action,
        session: anchor.session.clone().unwrap_or_default(),
        content: json!({ "md_sha256": sha, "events": ids }).to_string(),
        tags: vec!["compress".into()],
        refs: ids_of(span),
        act: None,
        card: anchor.card.clone(),
    })?;
    rebuild_index(session_dir)?;
    Ok(path)
}

fn ids_of(span: &[&Memevent]) -> Vec<String> {
    span.iter().map(|e| e.id.clone()).collect()
}

fn uncompressed(events: &[Memevent]) -> Vec<&Memevent> {
    let start = events
        .iter()
        .rposition(|e| e.tags.iter().any(|t| t == "compress"))
        .map(|i| i + 1)
        .unwrap_or(0);
    events[start..]
        .iter()
        .filter(|e| !e.tags.iter().any(|t| t == "compress"))
        .collect()
}

fn push_piece(pieces: &mut Vec<Piece>, used: &mut usize, budget: usize, piece: Piece) -> bool {
    if piece.text.len() > budget.saturating_sub(*used) && !pieces.is_empty() {
        return false;
    }
    if piece.text.len() > budget && pieces.is_empty() {
        return false;
    }
    *used += piece.text.len();
    pieces.push(piece);
    true
}

fn sync_rows(events: &[Memevent], rows: Vec<Row>) -> Vec<Row> {
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

fn weak_of(rows: &[Row], sources: &[(String, f64)]) -> u64 {
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

fn md_dir(session_dir: &Path) -> PathBuf {
    session_dir.join("ctx/md")
}

fn index_path(session_dir: &Path) -> PathBuf {
    session_dir.join("ctx/index.jsonl")
}

fn load_rows(session_dir: &Path) -> Result<Vec<Row>, ContextError> {
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

fn save_rows(session_dir: &Path, rows: &[Row]) -> Result<(), ContextError> {
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

fn load_segments(session_dir: &Path) -> Result<Vec<Segment>, ContextError> {
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

fn read_proj(session_dir: &Path, name: &str) -> String {
    fs::read_to_string(session_dir.join(name)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::act::{FileFacet, MemoryFacet, Permit, ToolTag};
    use crate::instance::Steward;
    use crate::principal::card::{AgentCard, ModelSpec, ToolGrant, Topology};
    use crate::testkit::TempDir;
    use uuid::Uuid;

    fn card() -> AgentCard {
        AgentCard {
            v: 1,
            id: Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap(),
            model: ModelSpec {
                base_url: "https://example.com".into(),
                model: "m".into(),
                extra_body: None,
            },
            prompt: "prompt-text".into(),
            tools: vec![ToolGrant {
                name: "bash".into(),
                tag: ToolTag {
                    file: FileFacet::None,
                    permit: Permit::Deny,
                    memory: MemoryFacet::Ignore,
                },
            }],
            topology: Topology::Leaf,
            kernel: "0.1.0".into(),
            supersedes: None,
        }
    }

    fn session(tmp: &TempDir) -> PathBuf {
        let s = Steward::open(&card(), tmp.path(), "{\"k\":1}", "[\"c0\"]").unwrap();
        s.agent().session().root().to_path_buf()
    }

    #[test]
    fn summary_cites_events_and_second_cut_keeps_the_first_file() {
        let tmp = TempDir::new("ctx-cut");
        let dir = session(&tmp);
        let tape = JsonlStore::open(dir.join("memory.jsonl")).unwrap();
        let anchor = tape.read_all().unwrap().pop().unwrap();
        tape.append(AppendEvent {
            from: anchor.from.clone(),
            from_kind: FromKind::Agent,
            kind: Kind::Utterance,
            session: anchor.session.clone().unwrap_or_default(),
            content: "saw the bug".into(),
            tags: vec![],
            refs: vec![],
            act: None,
            card: anchor.card.clone(),
        })
        .unwrap();
        let first = record_summary(&dir, "user summary one").unwrap();
        let first_bytes = fs::read(&first).unwrap();
        let sources = segment_sources(&dir, &first).unwrap();
        let sum: f64 = sources.iter().map(|(_, c)| c).sum();
        assert!((sum - sources.len() as f64).abs() < 1e-6);
        assert!(sources.iter().any(|(_, c)| (*c - 1.0).abs() < 1e-9));
        tape.append(AppendEvent {
            from: anchor.from.clone(),
            from_kind: FromKind::Agent,
            kind: Kind::Utterance,
            session: anchor.session.clone().unwrap_or_default(),
            content: "later".into(),
            tags: vec![],
            refs: vec![],
            act: None,
            card: anchor.card.clone(),
        })
        .unwrap();
        let second = record_summary(&dir, "user summary two").unwrap();
        assert_ne!(first, second);
        assert_eq!(fs::read(&first).unwrap(), first_bytes);
    }

    #[test]
    fn continue_takes_recent_and_hot_and_bumps_weak_count() {
        let tmp = TempDir::new("ctx-hot");
        let dir = session(&tmp);
        let tape = JsonlStore::open(dir.join("memory.jsonl")).unwrap();
        let anchor = tape.read_all().unwrap().pop().unwrap();
        tape.append(AppendEvent {
            from: anchor.from.clone(),
            from_kind: FromKind::Agent,
            kind: Kind::Utterance,
            session: anchor.session.clone().unwrap_or_default(),
            content: "fact".into(),
            tags: vec![],
            refs: vec![],
            act: None,
            card: anchor.card.clone(),
        })
        .unwrap();
        record_summary(&dir, "sum").unwrap();
        tape.append(AppendEvent {
            from: anchor.from.clone(),
            from_kind: FromKind::Agent,
            kind: Kind::Utterance,
            session: anchor.session.clone().unwrap_or_default(),
            content: "tail".into(),
            tags: vec![],
            refs: vec![],
            act: None,
            card: anchor.card.clone(),
        })
        .unwrap();
        let got = assemble(&dir, AssembleMode::Continue, 10_000).unwrap();
        assert!(
            got.context
                .iter()
                .any(|p| p.kind == "recent" && p.text == "tail")
        );
        assert!(got.context.iter().any(|p| p.kind == "hot"));
        assert_eq!(
            got.prompt,
            tape.read_all()
                .unwrap()
                .iter()
                .rev()
                .find(|e| e.tags.iter().any(|t| t == "prompt"))
                .unwrap()
                .content
        );
        let rows = load_rows(&dir).unwrap();
        assert!(rows.iter().any(|r| r.weak >= 1));
    }

    #[test]
    fn length_cut_freezes_a_long_tail() {
        let tmp = TempDir::new("ctx-len");
        let dir = session(&tmp);
        let tape = JsonlStore::open(dir.join("memory.jsonl")).unwrap();
        let anchor = tape.read_all().unwrap().pop().unwrap();
        tape.append(AppendEvent {
            from: anchor.from,
            from_kind: FromKind::Agent,
            kind: Kind::Utterance,
            session: anchor.session.unwrap_or_default(),
            content: "x".repeat(40),
            tags: vec![],
            refs: vec![],
            act: None,
            card: anchor.card,
        })
        .unwrap();
        assert!(cut_if_long(&dir, 20).unwrap().is_some());
        assert!(cut_if_long(&dir, 20).unwrap().is_none());
    }

    #[test]
    fn independent_task_has_no_context() {
        let tmp = TempDir::new("ctx-empty");
        let dir = session(&tmp);
        record_summary(&dir, "sum").unwrap();
        let got = assemble(&dir, AssembleMode::Independent, 10_000).unwrap();
        assert!(got.context.is_empty());
        assert_eq!(got.config, "{\"k\":1}");
        assert!(!got.prompt.is_empty());
    }

    #[test]
    fn rebuild_restores_citations_after_the_sidecar_is_removed() {
        let tmp = TempDir::new("ctx-rebuild");
        let dir = session(&tmp);
        let tape = JsonlStore::open(dir.join("memory.jsonl")).unwrap();
        let anchor = tape.read_all().unwrap().pop().unwrap();
        tape.append(AppendEvent {
            from: anchor.from,
            from_kind: FromKind::Agent,
            kind: Kind::Utterance,
            session: anchor.session.unwrap_or_default(),
            content: "kept".into(),
            tags: vec![],
            refs: vec![],
            act: None,
            card: anchor.card,
        })
        .unwrap();
        let md = record_summary(&dir, "sum").unwrap();
        fs::remove_file(dir.join("ctx/index.jsonl")).unwrap();
        rebuild_index(&dir).unwrap();
        let rows = load_rows(&dir).unwrap();
        let sources = segment_sources(&dir, &md).unwrap();
        for (id, _) in sources {
            assert!(rows.iter().any(|r| r.event_id == id && r.segment.is_some()));
        }
    }
}
