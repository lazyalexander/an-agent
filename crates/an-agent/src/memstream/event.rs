use serde::{Deserialize, Serialize};

use crate::act::{ActEnvelope, Effect};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Utterance,
    Action,
    Observation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FromKind {
    Human,
    Agent,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Memevent {
    pub v: u32,
    pub id: String,
    pub seq: u64,
    pub ts: String,
    pub from: String,
    pub from_kind: FromKind,
    pub kind: Kind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    pub content: String,
    pub tags: Vec<String>,
    pub refs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub act: Option<ActOnEvent>,
}

/// Act envelope on a memevent. Older lines omit it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActOnEvent {
    pub kind: String,
    pub tag: crate::act::ToolTag,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workplace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<crate::workplace::Resource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effect: Option<Effect>,
}

impl ActOnEvent {
    pub fn intent(env: &ActEnvelope) -> Self {
        Self {
            kind: env.kind.as_str().to_string(),
            tag: env.tag.clone(),
            workplace: env.workplace.clone(),
            resource: env.resource.clone(),
            tool: env.tool.clone(),
            effect: None,
        }
    }

    pub fn with_effect(env: &ActEnvelope, effect: Effect) -> Self {
        Self {
            kind: env.kind.as_str().to_string(),
            tag: env.tag.clone(),
            workplace: env.workplace.clone(),
            resource: env.resource.clone(),
            tool: env.tool.clone(),
            effect: Some(effect),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppendEvent {
    pub from: String,
    pub from_kind: FromKind,
    pub kind: Kind,
    pub session: String,
    pub content: String,
    pub tags: Vec<String>,
    pub refs: Vec<String>,
    pub act: Option<ActOnEvent>,
}

impl Memevent {
    pub fn validate(&self) -> Result<(), String> {
        if self.v != 1 {
            return Err("memory event v must be 1".into());
        }
        if self.id.is_empty() {
            return Err("memory event id must be a string".into());
        }
        Ok(())
    }
}
