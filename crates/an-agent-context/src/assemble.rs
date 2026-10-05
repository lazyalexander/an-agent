//! Cut a long tail and assemble pieces for one session directory.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde_json::json;
use sha2::{Digest, Sha256};

use an_agent_core::memstream::Memevent;

use super::index::{
    Segment, index_path, load_rows, load_segments, md_dir, rebuild_index, save_rows, sync_rows,
    weak_of,
};
use super::{AssembleMode, Assembly, ContextError, Cut, Piece, read_proj};

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

pub(super) fn write_cut(session_dir: &Path, span: &[&Memevent]) -> Result<Cut, ContextError> {
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

pub(super) fn uncompressed(events: &[Memevent]) -> Vec<&Memevent> {
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
