mod charter;
mod registry;
mod resource;
mod sense;
mod tag;
mod tool;

pub use charter::{Audience, Charter, Signal};
pub use registry::{Registration, RegistryError, Tier, ToolRegistry};
pub use resource::{Resource, ResourceKind};
pub use sense::{Effect, effect_from_tag};
pub use tag::{FileFacet, MemoryFacet, Permit, ToolTag};
pub use tool::{
    ActCtx, Tool, ToolActResult, ToolCall, ToolCtx, ToolError, ToolMessage, run_tool_act, tag_of,
};

/// Intent-domain vocabulary only. The channel (via tool / direct / model)
/// is carried by `ActEnvelope.tool`: Some(name) = via tool, None = direct
/// or model-side. Invoke covers all non-deterministic external calls —
/// generic tool use and model calls alike.
///
/// `Deny` is the agent-side refusal. Workspace rights use the word Forbidden
/// when an effect falls outside the workspace; that word is not an `ActKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActKind {
    Utterance,
    Invoke,
    Mount,
    Unmount,
    Deny,
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
            Self::Deny => "deny",
            Self::Allow => "allow",
            Self::Remember => "remember",
            Self::Forget => "forget",
        }
    }
}

/// What an act records. `permit` is this act's grant. File and memory
/// ceilings live on the tool tag and on the spawn charter, not here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActEnvelope {
    pub kind: ActKind,
    pub permit: Permit,
    pub tool: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn senses_file_faces() {
        let none = effect_from_tag(&ToolTag::none(), None);
        assert!(!none.unbounded);
        assert!(none.reads.is_empty());
        let read = effect_from_tag(&ToolTag::read("/src/a.ts"), Some("wp".into()));
        assert_eq!(read.reads, vec!["/src/a.ts".to_string()]);
        assert_eq!(read.workplace.as_deref(), Some("wp"));
        let write = effect_from_tag(&ToolTag::write("/src/a.ts"), None);
        assert_eq!(write.writes, vec!["/src/a.ts".to_string()]);
        assert!(write.workplace.is_none());
        assert!(effect_from_tag(&ToolTag::unbounded(), None).unbounded);
    }
}
