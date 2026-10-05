//! Thread-facing cuts and assembly. Tape writes go through `AgentControl`.

use std::path::Path;

use serde_json::{Value, json};
use uuid::Uuid;

use an_agent_core::control::AgentControl;
use an_agent_core::memstream::Memevent;

use super::assemble::{assemble, cut_if_long, uncompressed, write_cut};
use super::index::{load_segments, rebuild_index};
use super::{AssembleMode, Assembly, ContextError, Cut, Piece, read_proj};

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
