use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::act::ToolTag;

/// Model access spec. Never carries the api_key — keys come from
/// env/config at build time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelSpec {
    pub base_url: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_body: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolGrant {
    pub name: String,
    pub tag: ToolTag,
}

/// Composite shape. Only Leaf exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Topology {
    Leaf,
}

/// Complete, immutable identity of an agent: id, model spec, prompt,
/// permission grants, topology, kernel reference, and the supersedes edge.
/// Memory (tape/session/clips), api keys, and runtime deps (clock,
/// entropy, store) are NOT part of the card — they are injected at build.
///
/// Hash rules (tape-evolution E1): the hash is sha256 of the serde_json
/// serialization, deterministic because struct field order is fixed.
/// Changing field order or field semantics changes hashes; treat such
/// changes as a new schema `v`, additive only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentCard {
    pub v: u32,
    pub id: Uuid,
    pub model: ModelSpec,
    pub prompt: String,
    pub tools: Vec<ToolGrant>,
    pub topology: Topology,
    pub kernel: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<String>,
}

impl AgentCard {
    pub fn hash(&self) -> Result<String, serde_json::Error> {
        let bytes = serde_json::to_vec(self)?;
        Ok(Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::act::ToolTag;

    fn card() -> AgentCard {
        AgentCard {
            v: 1,
            id: Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap(),
            model: ModelSpec {
                base_url: "https://api.deepseek.com".into(),
                model: "deepseek-v4-flash".into(),
                extra_body: None,
            },
            prompt: "You are a helpful assistant.".into(),
            tools: vec![ToolGrant {
                name: "bash".into(),
                tag: ToolTag::none(),
            }],
            topology: Topology::Leaf,
            kernel: "0.1.0".into(),
            supersedes: None,
        }
    }

    #[test]
    fn same_card_same_hash_and_round_trip() {
        let a = card();
        let b = card();
        assert_eq!(a.hash().unwrap(), b.hash().unwrap());
        let json = serde_json::to_string(&a).unwrap();
        let back: AgentCard = serde_json::from_str(&json).unwrap();
        assert_eq!(back, a);
        assert_eq!(back.hash().unwrap(), a.hash().unwrap());
    }

    #[test]
    fn any_field_change_changes_hash() {
        let base = card();
        let mut renamed = card();
        renamed.prompt = "other".into();
        assert_ne!(renamed.hash().unwrap(), base.hash().unwrap());
        let mut successor = card();
        successor.supersedes = Some(base.hash().unwrap());
        assert_ne!(successor.hash().unwrap(), base.hash().unwrap());
    }
}
