use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::act::{ActEnvelope, Effect, Permit};

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<String>,
}

/// Act envelope on a memevent. Older lines omit it. Lines written before
/// the permit field stored that permit inside `tag`; the rest of `tag` is
/// ignored on read and is not written again.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ActOnEvent {
    pub kind: String,
    pub permit: Permit,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effect: Option<Effect>,
}

#[derive(Deserialize)]
struct ActOnEventDe {
    kind: String,
    #[serde(default)]
    permit: Option<Permit>,
    #[serde(default)]
    tag: Option<LegacyTagPermit>,
    #[serde(default)]
    tool: Option<String>,
    #[serde(default)]
    effect: Option<Effect>,
}

#[derive(Deserialize)]
struct LegacyTagPermit {
    permit: Permit,
}

impl<'de> Deserialize<'de> for ActOnEvent {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = ActOnEventDe::deserialize(deserializer)?;
        let permit = raw
            .permit
            .or_else(|| raw.tag.map(|tag| tag.permit))
            .ok_or_else(|| D::Error::custom("act requires permit"))?;
        Ok(Self {
            kind: raw.kind,
            permit,
            tool: raw.tool,
            effect: raw.effect,
        })
    }
}

impl ActOnEvent {
    pub fn intent(env: &ActEnvelope) -> Self {
        Self {
            kind: env.kind.as_str().to_string(),
            permit: env.permit,
            tool: env.tool.clone(),
            effect: None,
        }
    }

    pub fn with_effect(env: &ActEnvelope, effect: Effect) -> Self {
        Self {
            kind: env.kind.as_str().to_string(),
            permit: env.permit,
            tool: env.tool.clone(),
            effect: Some(effect),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_tag_permit_still_reads_and_is_not_written() {
        let raw = r#"{"kind":"mount","tag":{"form":"bare","permit":"deny","file":"none","memory":"ignore"},"tool":"clerk"}"#;
        let act: ActOnEvent = serde_json::from_str(raw).unwrap();
        assert_eq!(act.permit, Permit::Deny);
        assert_eq!(act.tool.as_deref(), Some("clerk"));
        let out = serde_json::to_value(&act).unwrap();
        assert!(out.get("tag").is_none());
        assert_eq!(out["permit"], "deny");
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
    pub card: Option<String>,
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
