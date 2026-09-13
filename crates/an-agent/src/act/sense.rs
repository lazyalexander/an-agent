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
        },
        FileFacet::Unbounded => Effect {
            reads: vec![],
            writes: vec![],
            unbounded: true,
            workplace,
            memory,
        },
        FileFacet::Read { path, .. } => Effect {
            reads: vec![path.clone()],
            writes: vec![],
            unbounded: false,
            workplace,
            memory,
        },
        FileFacet::Write { path, .. } => Effect {
            reads: vec![],
            writes: vec![path.clone()],
            unbounded: false,
            workplace,
            memory,
        },
        FileFacet::ReadWrite { path, .. } => Effect {
            reads: vec![path.clone()],
            writes: vec![path.clone()],
            unbounded: false,
            workplace,
            memory,
        },
    }
}
