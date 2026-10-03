//! Two-layer context for one thread tape. The tape stays the record.
//! Compressed markdown cites event ids. The sidecar index can be deleted
//! and rebuilt. This crate writes `ctx/` only. `AgentControl` writes the tape.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use an_agent_core::control::{AgentControl, ControlError};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use an_agent_core::memstream::Memevent;

#[derive(Debug, Error)]
pub enum ContextError {
    #[error("store: {0}")]
    Store(#[from] an_agent_core::memstream::StoreError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("session has no events")]
    Empty,
    #[error("md missing sources")]
    BadMd,
    #[error(transparent)]
    Control(#[from] ControlError),
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
    /// An id that is not on this thread. Assemble does not set it.
    /// Workspace config and env generations use this later.
    pub outside: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cut {
    pub path: PathBuf,
    pub sha256: String,
    pub events: Vec<String>,
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

/// Freeze the uncompressed tail when its text is at least `max_chars`.
/// Does not write the tape.
pub fn cut_if_long(
    session_dir: &Path,
    events: &[Memevent],
    max_chars: usize,
) -> Result<Option<Cut>, ContextError> {
    let span = uncompressed(events);
    let n: usize = span.iter().map(|e| e.content.len()).sum();
    if n < max_chars || span.is_empty() {
        return Ok(None);
    }
    Ok(Some(write_cut(session_dir, &span)?))
}

pub fn assemble(
    session_dir: &Path,
    events: &[Memevent],
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
        rebuild_index(session_dir, events)?;
    }
    let mut rows = sync_rows(events, load_rows(session_dir)?);
    let segments = load_segments(session_dir)?;
    let recent: Vec<&Memevent> = uncompressed(events);
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
                outside: None,
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
                outside: None,
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
                outside: None,
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

fn write_cut(session_dir: &Path, span: &[&Memevent]) -> Result<Cut, ContextError> {
    if span.is_empty() {
        return Err(ContextError::Empty);
    }
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
    Ok(Cut {
        path,
        sha256: sha,
        events: ids,
    })
}

/// Note a summary, cut the tail, and record the cut. The tape writes go
/// through `AgentControl`.
pub fn summarize_thread(
    control: &AgentControl,
    thread: Uuid,
    text: &str,
) -> Result<Cut, ContextError> {
    control.note(thread, "summary", text)?;
    cut_thread_since(control, thread)
}

/// Cut the uncompressed tail when it is long enough. Short tails return
/// `Ok(None)` and write nothing.
pub fn cut_thread(
    control: &AgentControl,
    thread: Uuid,
    max_chars: usize,
) -> Result<Option<Cut>, ContextError> {
    let dir = control.directory(thread)?;
    let events = control.events(thread)?;
    let Some(cut) = cut_if_long(&dir, &events, max_chars)? else {
        return Ok(None);
    };
    control.commit_compress(thread, &cut.sha256, &cut.events)?;
    let events = control.events(thread)?;
    rebuild_index(&dir, &events)?;
    Ok(Some(cut))
}

/// Choose pieces for this thread. A second call with the same mode, the
/// same budget, and no new tape events returns the same pieces and does
/// not rewrite markdown or the index.
pub fn assemble_thread(
    control: &AgentControl,
    thread: Uuid,
    mode: AssembleMode,
    budget_chars: usize,
) -> Result<Assembly, ContextError> {
    let dir = control.directory(thread)?;
    let prompt = control.prompt(thread)?;
    let config = read_proj(&dir, "config");
    if mode == AssembleMode::Independent {
        return Ok(Assembly {
            prompt,
            config,
            context: Vec::new(),
        });
    }
    let events = control.events(thread)?;
    let cite = control.workspace_cite()?;
    if let Some(hit) = cache_hit(&dir, &events, mode, budget_chars, &cite)? {
        return Ok(Assembly {
            prompt,
            config,
            context: hit,
        });
    }
    let mut assembly = assemble(&dir, &events, mode, budget_chars)?;
    assembly.prompt = prompt;
    assembly.config = if config.is_empty() {
        assembly.config
    } else {
        config
    };
    if let Some(id) = &cite.config_id {
        assembly.context.push(Piece {
            kind: "config",
            event_id: None,
            segment: None,
            outside: Some(id.clone()),
            text: String::new(),
        });
    }
    if let Some(id) = &cite.env_id {
        assembly.context.push(Piece {
            kind: "env",
            event_id: None,
            segment: None,
            outside: Some(id.clone()),
            text: cite.env_markdown.clone(),
        });
    }
    let refs = piece_refs(&assembly.context);
    let body = json!({
        "mode": mode_name(mode),
        "budget_chars": budget_chars,
        "config_id": cite.config_id,
        "env_id": cite.env_id,
        "pieces": assembly.context.iter().map(piece_body).collect::<Vec<_>>(),
    })
    .to_string();
    control.commit_context(thread, &body, &refs)?;
    Ok(assembly)
}

fn cut_thread_since(control: &AgentControl, thread: Uuid) -> Result<Cut, ContextError> {
    let dir = control.directory(thread)?;
    let events = control.events(thread)?;
    let span = uncompressed(&events);
    let cut = write_cut(&dir, &span)?;
    control.commit_compress(thread, &cut.sha256, &cut.events)?;
    let events = control.events(thread)?;
    rebuild_index(&dir, &events)?;
    Ok(cut)
}

fn mode_name(mode: AssembleMode) -> &'static str {
    match mode {
        AssembleMode::Continue => "continue",
        AssembleMode::Independent => "independent",
    }
}

fn piece_refs(pieces: &[Piece]) -> Vec<String> {
    pieces
        .iter()
        .filter_map(|piece| {
            piece
                .event_id
                .clone()
                .or_else(|| piece.segment.clone())
                .or_else(|| piece.outside.clone())
        })
        .collect()
}

fn piece_body(piece: &Piece) -> Value {
    json!({
        "kind": piece.kind,
        "event_id": piece.event_id,
        "segment": piece.segment,
        "outside": piece.outside,
    })
}

fn cache_hit(
    dir: &Path,
    events: &[Memevent],
    mode: AssembleMode,
    budget_chars: usize,
    cite: &an_agent_core::workspace::WorkspaceCite,
) -> Result<Option<Vec<Piece>>, ContextError> {
    let Some(pos) = events
        .iter()
        .rposition(|event| event.tags.iter().any(|tag| tag == "context"))
    else {
        return Ok(None);
    };
    if pos + 1 != events.len() {
        return Ok(None);
    }
    let body: Value = serde_json::from_str(&events[pos].content)?;
    if body["mode"] != mode_name(mode) || body["budget_chars"] != budget_chars {
        return Ok(None);
    }
    if body["config_id"].as_str() != cite.config_id.as_deref()
        || body["env_id"].as_str() != cite.env_id.as_deref()
    {
        return Ok(None);
    }
    let Some(listed) = body["pieces"].as_array() else {
        return Ok(None);
    };
    let mut pieces = Vec::new();
    for item in listed {
        let kind = item["kind"].as_str().unwrap_or("");
        let event_id = item["event_id"].as_str().map(str::to_string);
        let segment = item["segment"].as_str().map(str::to_string);
        let outside = item["outside"].as_str().map(str::to_string);
        let text = if kind == "config" {
            String::new()
        } else if kind == "env" {
            cite.env_markdown.clone()
        } else if let Some(id) = &event_id {
            let Some(event) = events.iter().find(|event| event.id == *id) else {
                return Ok(None);
            };
            event.content.clone()
        } else if let Some(id) = &segment {
            let Some(seg) = load_segments(dir)?.into_iter().find(|seg| seg.id == *id) else {
                return Ok(None);
            };
            seg.text
        } else if outside.is_some() {
            String::new()
        } else {
            return Ok(None);
        };
        pieces.push(Piece {
            kind: match kind {
                "recent" => "recent",
                "hot" => "hot",
                "summary" => "summary",
                "config" => "config",
                "env" => "env",
                _ => return Ok(None),
            },
            event_id,
            segment,
            outside,
            text,
        });
    }
    Ok(Some(pieces))
}

fn uncompressed(events: &[Memevent]) -> Vec<&Memevent> {
    let start = events
        .iter()
        .rposition(|e| e.tags.iter().any(|t| t == "compress"))
        .map(|i| i + 1)
        .unwrap_or(0);
    events[start..]
        .iter()
        .filter(|event| {
            !event.tags.iter().any(|tag| {
                matches!(
                    tag.as_str(),
                    "compress" | "context" | "thread" | "cancel" | "finish" | "send"
                )
            })
        })
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
    use an_agent_core::act::{FileFacet, MemoryFacet, Permit, ToolTag};
    use an_agent_core::control::AgentControl;
    use an_agent_core::memstream::{AppendEvent, FromKind, JsonlStore, Kind};
    use an_agent_core::principal::card::{AgentCard, ModelSpec, ToolGrant, Topology};
    use an_agent_core::recorder::Recorder;
    use an_agent_core::testkit::{TempDir, bash_registry};
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

    fn seal(store: &JsonlStore, dir: &Path, summary: &str) -> PathBuf {
        let anchor = store.read_all().unwrap().pop().unwrap();
        store
            .append(AppendEvent {
                from: anchor.from.clone(),
                from_kind: FromKind::Agent,
                kind: Kind::Utterance,
                session: anchor.session.clone().unwrap_or_default(),
                content: summary.to_string(),
                tags: vec!["summary".into()],
                refs: vec![],
                act: None,
                card: anchor.card.clone(),
            })
            .unwrap();
        let events = store.read_all().unwrap();
        let cut = cut_if_long(dir, &events, 0).unwrap().unwrap();
        store
            .append(AppendEvent {
                from: anchor.from,
                from_kind: FromKind::Agent,
                kind: Kind::Action,
                session: anchor.session.unwrap_or_default(),
                content: json!({ "md_sha256": cut.sha256, "events": cut.events }).to_string(),
                tags: vec!["compress".into()],
                refs: cut.events,
                act: None,
                card: anchor.card,
            })
            .unwrap();
        rebuild_index(dir, &store.read_all().unwrap()).unwrap();
        cut.path
    }

    fn session(tmp: &TempDir) -> PathBuf {
        let s = Recorder::open(
            &card(),
            tmp.path(),
            "{\"k\":1}",
            "[\"c0\"]",
            &an_agent_factory::tool_registry(),
        )
        .unwrap();
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
        let first = seal(&tape, &dir, "user summary one");
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
        let second = seal(&tape, &dir, "user summary two");
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
        seal(&tape, &dir, "sum");
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
        let got = assemble(
            &dir,
            &tape.read_all().unwrap(),
            AssembleMode::Continue,
            10_000,
        )
        .unwrap();
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
        let events = tape.read_all().unwrap();
        let cut = cut_if_long(&dir, &events, 20).unwrap().unwrap();
        tape.append(AppendEvent {
            from: "a".into(),
            from_kind: FromKind::Agent,
            kind: Kind::Action,
            session: "s".into(),
            content: json!({ "md_sha256": cut.sha256, "events": cut.events }).to_string(),
            tags: vec!["compress".into()],
            refs: cut.events.clone(),
            act: None,
            card: None,
        })
        .unwrap();
        let bytes = fs::read(&cut.path).unwrap();
        let again = cut_if_long(&dir, &tape.read_all().unwrap(), 20).unwrap();
        assert!(again.is_none());
        assert_eq!(fs::read(&cut.path).unwrap(), bytes);
    }

    #[test]
    fn independent_task_has_no_context() {
        let tmp = TempDir::new("ctx-empty");
        let dir = session(&tmp);
        let tape = JsonlStore::open(dir.join("memory.jsonl")).unwrap();
        seal(&tape, &dir, "sum");
        let got = assemble(
            &dir,
            &tape.read_all().unwrap(),
            AssembleMode::Independent,
            10_000,
        )
        .unwrap();
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
        let md = seal(&tape, &dir, "sum");
        fs::remove_file(dir.join("ctx/index.jsonl")).unwrap();
        rebuild_index(&dir, &tape.read_all().unwrap()).unwrap();
        let rows = load_rows(&dir).unwrap();
        let sources = segment_sources(&dir, &md).unwrap();
        for (id, _) in sources {
            assert!(rows.iter().any(|r| r.event_id == id && r.segment.is_some()));
        }
    }

    fn thread_card(id: &str) -> AgentCard {
        AgentCard {
            v: 1,
            id: Uuid::parse_str(id).unwrap(),
            model: ModelSpec {
                base_url: "https://example.com".into(),
                model: "m".into(),
                extra_body: None,
            },
            prompt: "prompt-text".into(),
            tools: vec![ToolGrant {
                name: "bash".into(),
                tag: ToolTag::none_permit(Permit::Deny),
            }],
            topology: Topology::Leaf,
            kernel: "0.1.0".into(),
            supersedes: None,
        }
    }

    #[test]
    fn thread_cut_is_written_by_the_host_and_a_hit_does_not_rewrite() {
        let tmp = TempDir::new("ctx-thread");
        let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
        let id = control
            .open_thread(&thread_card("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"))
            .unwrap();
        control.advance(id, "hello from the host").unwrap();
        let cut = summarize_thread(&control, id, "the summary").unwrap();
        let md = fs::read(&cut.path).unwrap();
        let first = assemble_thread(&control, id, AssembleMode::Continue, 10_000).unwrap();
        assert!(
            first
                .context
                .iter()
                .any(|piece| piece.text == "the summary")
        );
        assert_eq!(first.prompt, "prompt-text");
        assert!(first.context.iter().all(|piece| piece.outside.is_none()));
        let index = fs::read(control.directory(id).unwrap().join("ctx/index.jsonl")).unwrap();
        let second = assemble_thread(&control, id, AssembleMode::Continue, 10_000).unwrap();
        assert_eq!(second.context, first.context);
        assert_eq!(fs::read(&cut.path).unwrap(), md);
        assert_eq!(
            fs::read(control.directory(id).unwrap().join("ctx/index.jsonl")).unwrap(),
            index
        );
        let alone = assemble_thread(&control, id, AssembleMode::Independent, 10_000).unwrap();
        assert!(alone.context.is_empty());
        assert_eq!(alone.prompt, "prompt-text");
        let config = control.put_config(b"{\"theme\":\"quiet\"}").unwrap();
        let env = control.put_env("# now\n\nprefers short diffs\n").unwrap();
        let cited = assemble_thread(&control, id, AssembleMode::Continue, 10_000).unwrap();
        assert!(cited.context.iter().any(|piece| {
            piece.kind == "config" && piece.outside.as_deref() == Some(config.id.as_str())
        }));
        assert!(cited.context.iter().any(|piece| {
            piece.kind == "env"
                && piece.outside.as_deref() == Some(env.id.as_str())
                && piece.text.contains("short diffs")
        }));
        let again = assemble_thread(&control, id, AssembleMode::Continue, 10_000).unwrap();
        assert_eq!(again.context, cited.context);
        control.put_env("# now\n\nprefers tests first\n").unwrap();
        let moved = assemble_thread(&control, id, AssembleMode::Continue, 10_000).unwrap();
        assert!(
            moved
                .context
                .iter()
                .any(|piece| piece.text.contains("tests first"))
        );
        assert_ne!(moved.context, cited.context);
    }
}
