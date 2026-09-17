mod sense;
mod sentence;
mod tag;
mod tool;

pub use sense::{Effect, effect_from_sentence, effect_from_tag};
pub use sentence::{Access, ActSentence, BareFile, Ingest, SentenceError};
pub use tag::{FileFacet, MemoryFacet, Permit, ToolTag};
pub use tool::{ActCtx, Tool, ToolCall, ToolCtx, ToolError, ToolMessage, run_tool_act, tag_of};

/// Intent-domain vocabulary only. The channel (via tool / direct / model)
/// is carried by `ActEnvelope.tool`: Some(name) = via tool, None = direct
/// or model-side. Invoke covers all non-deterministic external calls —
/// generic tool use and model calls alike.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActKind {
    Utterance,
    Invoke,
    Mount,
    Unmount,
    Forbid,
    Allow,
    Remember,
    Forget,
}

impl ActKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Utterance => "utterance",
            Self::Invoke => "invoke",
            Self::Mount => "mount",
            Self::Unmount => "unmount",
            Self::Forbid => "forbid",
            Self::Allow => "allow",
            Self::Remember => "remember",
            Self::Forget => "forget",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActEnvelope {
    pub kind: ActKind,
    pub sentence: ActSentence,
    pub tool: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn senses_file_faces() {
        let none = effect_from_sentence(&ActSentence::from_seed(&ToolTag::none(), None).unwrap());
        assert!(!none.unbounded);
        assert!(none.reads.is_empty());
        let wp = uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap();
        let res = crate::workplace::Resource::new(
            wp,
            crate::workplace::ResourceKind::File,
            ["src", "a.ts"],
        );
        let read = effect_from_sentence(
            &ActSentence::from_seed(&ToolTag::read("/src/a.ts"), Some(res.clone())).unwrap(),
        );
        assert!(read.reads.iter().any(|p| p.ends_with("/src/a.ts")));
        let write = effect_from_sentence(
            &ActSentence::from_seed(&ToolTag::write("/src/a.ts"), Some(res)).unwrap(),
        );
        assert!(write.writes.iter().any(|p| p.ends_with("/src/a.ts")));
        assert!(
            effect_from_sentence(&ActSentence::from_seed(&ToolTag::unbounded(), None).unwrap())
                .unbounded
        );
    }
}
