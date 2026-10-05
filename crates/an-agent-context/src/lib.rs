//! Two-layer context for one thread tape. The tape stays the record.
//! Compressed markdown cites event ids. The sidecar index can be deleted
//! and rebuilt. This crate writes `ctx/` only. `AgentControl` writes the tape.

use std::fs;
use std::path::{Path, PathBuf};

use an_agent_core::control::ControlError;
use thiserror::Error;

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
    /// An id that is not on this thread. Continue assembly sets it for the
    /// current workspace config and env. Tape pieces leave it empty.
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

mod assemble;
mod index;
mod thread;

pub use assemble::{assemble, cut_if_long};
pub use index::{rebuild_index, segment_sources};
pub use thread::{assemble_thread, cut_thread, summarize_thread};

fn read_proj(session_dir: &Path, name: &str) -> String {
    fs::read_to_string(session_dir.join(name)).unwrap_or_default()
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
