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

pub fn effect_from_tag(tag: &ToolTag, workplace: Option<String>) -> Effect {
    let memory = tag.memory.op_name().to_string();
    match &tag.file {
        FileFacet::None => Effect {
            reads: vec![],
            writes: vec![],
            unbounded: false,
            workplace,
            memory,
            from_head: None,
            to_head: None,
            blob: None,
        },
        FileFacet::Unbounded => Effect {
            reads: vec![],
            writes: vec![],
            unbounded: true,
            workplace,
            memory,
            from_head: None,
            to_head: None,
            blob: None,
        },
        FileFacet::Read { path, .. } => Effect {
            reads: vec![path.clone()],
            writes: vec![],
            unbounded: false,
            workplace,
            memory,
            from_head: None,
            to_head: None,
            blob: None,
        },
        FileFacet::Write { path, .. } => Effect {
            reads: vec![],
            writes: vec![path.clone()],
            unbounded: false,
            workplace,
            memory,
            from_head: None,
            to_head: None,
            blob: None,
        },
        FileFacet::ReadWrite { path, .. } => Effect {
            reads: vec![path.clone()],
            writes: vec![path.clone()],
            unbounded: false,
            workplace,
            memory,
            from_head: None,
            to_head: None,
            blob: None,
        },
    }
}
