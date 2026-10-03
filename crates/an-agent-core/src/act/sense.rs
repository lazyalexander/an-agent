use serde::{Deserialize, Serialize};

use super::tag::{FileFacet, ToolTag};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Effect {
    pub reads: Vec<String>,
    pub writes: Vec<String>,
    pub unbounded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workplace: Option<String>,
    pub memory: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob: Option<String>,
}

/// Project an effect from the tool tag. A path on the tag is the effect
/// path. No workspace resource is required, and a missing one is not a refusal.
pub fn effect_from_tag(tag: &ToolTag, workplace: Option<String>) -> Effect {
    let (reads, writes, unbounded) = match &tag.file {
        FileFacet::None => (Vec::new(), Vec::new(), false),
        FileFacet::Unbounded => (Vec::new(), Vec::new(), true),
        FileFacet::Read { path, .. } => (vec![path.clone()], Vec::new(), false),
        FileFacet::Write { path, .. } => (Vec::new(), vec![path.clone()], false),
        FileFacet::ReadWrite { path, .. } => (vec![path.clone()], vec![path.clone()], false),
    };
    Effect {
        reads,
        writes,
        unbounded,
        workplace,
        memory: tag.memory.op_name().into(),
        from_head: None,
        to_head: None,
        blob: None,
    }
}
