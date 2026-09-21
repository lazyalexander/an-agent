//! Rebuild prompt/config/context projections from the session tape.
//! Missing source events are an error — the prompt is not invented.

use std::fs;
use std::path::Path;

use thiserror::Error;

use crate::memstream::JsonlStore;

use super::steward::projection_path;

#[derive(Debug, Error)]
pub enum RecoverError {
    #[error("store: {0}")]
    Store(#[from] crate::memstream::StoreError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("tape has no {0} projection to rebuild")]
    Missing(&'static str),
}

/// Session directory (the one that contains `memory.jsonl`). Existing
/// projection files are left in place. Absent files are rewritten from the
/// last matching tagged event.
pub fn recover(session_dir: &Path) -> Result<(), RecoverError> {
    let tape = JsonlStore::open(session_dir.join("memory.jsonl"))?;
    let events = tape.read_all()?;
    restore(&events, session_dir, "prompt")?;
    restore(&events, session_dir, "config")?;
    restore(&events, session_dir, "context")?;
    Ok(())
}

fn restore(
    events: &[crate::memstream::Memevent],
    session_dir: &Path,
    name: &'static str,
) -> Result<(), RecoverError> {
    let path = projection_path(session_dir, name);
    if path.exists() {
        return Ok(());
    }
    let content = events
        .iter()
        .rev()
        .find(|e| e.tags.iter().any(|t| t == name))
        .map(|e| e.content.clone())
        .ok_or(RecoverError::Missing(name))?;
    if name == "prompt" && content.is_empty() {
        return Err(RecoverError::Missing(name));
    }
    fs::write(path, content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::act::Permit;
    use crate::act::{FileFacet, MemoryFacet, ToolTag};
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
            prompt: "original-prompt".into(),
            tools: vec![ToolGrant {
                name: "bash".into(),
                tag: ToolTag {
                    file: FileFacet::None,
                    permit: Permit::Forbidden,
                    memory: MemoryFacet::Ignore,
                },
            }],
            topology: Topology::Leaf,
            kernel: "0.1.0".into(),
            supersedes: None,
        }
    }

    #[test]
    fn rebuilds_absent_projections_from_tape() {
        let tmp = TempDir::new("recover-ok");
        let steward = Steward::open(&card(), tmp.path(), "{\"a\":1}", "[\"c1\"]").unwrap();
        let dir = steward.agent().session().root().to_path_buf();
        let hash = steward.agent().card_hash().to_string();
        fs::remove_file(dir.join("prompt")).unwrap();
        fs::remove_file(dir.join("config")).unwrap();
        fs::remove_file(dir.join("context")).unwrap();
        recover(&dir).unwrap();
        assert_eq!(fs::read_to_string(dir.join("prompt")).unwrap(), hash);
        assert_eq!(fs::read_to_string(dir.join("config")).unwrap(), "{\"a\":1}");
        assert_eq!(fs::read_to_string(dir.join("context")).unwrap(), "[\"c1\"]");
        assert!(
            !fs::read_to_string(dir.join("prompt"))
                .unwrap()
                .contains("original-prompt")
        );
    }

    #[test]
    fn missing_card_hash_event_fails_closed() {
        let tmp = TempDir::new("recover-miss");
        let agent_id = Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap();
        let session = crate::instance::Session::create(tmp.path(), agent_id).unwrap();
        let err = recover(session.root()).unwrap_err();
        assert!(matches!(err, RecoverError::Missing("prompt")));
    }
}
