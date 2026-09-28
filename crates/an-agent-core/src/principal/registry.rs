//! Append-only registry of agent cards. Layout follows the workplace HEAD
//! idiom without depending on workplace:
//! `<root>/agents/<id>/cards/<hash>.json` + `<root>/agents/<id>/HEAD`.

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;
use uuid::Uuid;

use super::card::AgentCard;

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("card {0} not found for agent {1}")]
    NotFound(String, String),
    #[error("lineage broken at {0}: card missing")]
    BrokenLineage(String),
    #[error("invalid agent id (want a UUID): {0}")]
    InvalidId(String),
    #[error("invalid card hash (want 64 lowercase hex): {0}")]
    InvalidHash(String),
}

// id and hash join into filesystem paths, and `Path::join` honors `..`:
// without a strict alphabet a caller could escape the agent prefix
// (e.g. card(root, "../../../tmp", "x")). Only the shapes register() itself
// produces are accepted — canonical UUID, sha256 lowercase hex.
fn check_agent_id(id: &str) -> Result<(), RegistryError> {
    if Uuid::parse_str(id).is_err() {
        return Err(RegistryError::InvalidId(id.into()));
    }
    Ok(())
}

fn check_hash(hash: &str) -> Result<(), RegistryError> {
    let ok = hash.len() == 64
        && hash
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c));
    if !ok {
        return Err(RegistryError::InvalidHash(hash.into()));
    }
    Ok(())
}

fn agent_dir(root: &Path, id: &str) -> PathBuf {
    root.join("agents").join(id)
}

fn cards_dir(root: &Path, id: &str) -> PathBuf {
    agent_dir(root, id).join("cards")
}

/// Writes the card (if new) and moves HEAD. Never rewrites or deletes.
pub fn register(root: &Path, card: &AgentCard) -> Result<String, RegistryError> {
    let hash = card.hash()?;
    let dir = cards_dir(root, &card.id.to_string());
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{hash}.json"));
    if !path.exists() {
        fs::write(&path, serde_json::to_string_pretty(card)?)?;
    }
    fs::write(
        agent_dir(root, &card.id.to_string()).join("HEAD"),
        format!("{hash}\n"),
    )?;
    Ok(hash)
}

pub fn head(root: &Path, id: &str) -> Result<Option<String>, RegistryError> {
    check_agent_id(id)?;
    let path = agent_dir(root, id).join("HEAD");
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(fs::read_to_string(path)?.trim().to_string()))
}

pub fn card(root: &Path, id: &str, hash: &str) -> Result<AgentCard, RegistryError> {
    check_agent_id(id)?;
    check_hash(hash)?;
    let path = cards_dir(root, id).join(format!("{hash}.json"));
    if !path.exists() {
        return Err(RegistryError::NotFound(hash.into(), id.into()));
    }
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

/// HEAD-first lineage, walking supersedes edges back to the root card.
pub fn lineage(root: &Path, id: &str) -> Result<Vec<String>, RegistryError> {
    let mut chain = Vec::new();
    let mut cursor = head(root, id)?;
    while let Some(hash) = cursor {
        let c = card(root, id, &hash).map_err(|_| RegistryError::BrokenLineage(hash.clone()))?;
        chain.push(hash);
        cursor = c.supersedes;
    }
    Ok(chain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::act::ToolTag;
    use crate::principal::card::{ModelSpec, ToolGrant, Topology};
    use crate::testkit::TempDir;
    use uuid::Uuid;

    fn sample(supersedes: Option<String>) -> AgentCard {
        AgentCard {
            v: 1,
            id: Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap(),
            model: ModelSpec {
                base_url: "https://example.com".into(),
                model: "m".into(),
                extra_body: None,
            },
            prompt: "p".into(),
            tools: vec![ToolGrant {
                name: "bash".into(),
                tag: ToolTag::none(),
            }],
            topology: Topology::Leaf,
            kernel: "0.1.0".into(),
            supersedes,
        }
    }

    #[test]
    fn register_moves_head_and_round_trips() {
        let tmp = TempDir::new("reg");
        let c = sample(None);
        let hash = register(tmp.path(), &c).unwrap();
        let id = c.id.to_string();
        assert_eq!(head(tmp.path(), &id).unwrap(), Some(hash.clone()));
        let loaded = card(tmp.path(), &id, &hash).unwrap();
        assert_eq!(loaded, c);
    }

    #[test]
    fn supersession_builds_lineage_head_first() {
        let tmp = TempDir::new("reg");
        let c1 = sample(None);
        let h1 = register(tmp.path(), &c1).unwrap();
        let c2 = sample(Some(h1.clone()));
        let h2 = register(tmp.path(), &c2).unwrap();
        let id = c1.id.to_string();
        assert_eq!(head(tmp.path(), &id).unwrap(), Some(h2.clone()));
        assert_eq!(lineage(tmp.path(), &id).unwrap(), vec![h2, h1]);
    }

    #[test]
    fn unknown_agent_and_card_are_errors() {
        let tmp = TempDir::new("reg");
        let id = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
        assert_eq!(head(tmp.path(), id).unwrap(), None);
        let hash = "b".repeat(64);
        assert!(matches!(
            card(tmp.path(), id, &hash),
            Err(RegistryError::NotFound(..))
        ));
    }

    #[test]
    fn read_paths_reject_traversal_and_off_alphabet() {
        let tmp = TempDir::new("reg");
        let id = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
        assert!(matches!(
            head(tmp.path(), "../../../tmp"),
            Err(RegistryError::InvalidId(_))
        ));
        assert!(matches!(
            card(tmp.path(), id, "../../outside"),
            Err(RegistryError::InvalidHash(_))
        ));
        // Uppercase hex is off the digest alphabet too.
        assert!(matches!(
            card(tmp.path(), id, &"A".repeat(64)),
            Err(RegistryError::InvalidHash(_))
        ));
    }
}
