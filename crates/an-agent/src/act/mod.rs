mod sense;
mod tag;
mod tool;

pub use sense::{effect_from_tag, Effect};
pub use tag::{FileFacet, MemoryFacet, Permit, ToolTag};
pub use tool::{run_tool_act, tag_of, Tool, ToolCall, ToolCtx, ToolError, ToolMessage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActKind {
    Utterance,
    Tool,
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
            Self::Tool => "tool",
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
    pub tag: ToolTag,
    pub workplace: Option<String>,
    pub resource: Option<crate::workplace::Resource>,
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
        let read = effect_from_tag(&ToolTag::read("/src/a.ts"), None);
        assert_eq!(read.reads, ["/src/a.ts"]);
        let write = effect_from_tag(&ToolTag::write("/src/a.ts"), None);
        assert_eq!(write.writes, ["/src/a.ts"]);
        assert!(effect_from_tag(&ToolTag::unbounded(), None).unbounded);
    }
}
