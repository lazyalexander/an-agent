//! Bash is the one constructor this crate registers. The kernel builds
//! grants from whatever list the caller passes.

use std::sync::Arc;

use an_agent_core::principal::card::AgentCard;
use an_agent_core::principal::factory::{self, AgentRuntime, FactoryError, ToolCtor};

use crate::tools::Bash;

pub fn tool_registry() -> Vec<(&'static str, ToolCtor)> {
    vec![("bash", || Arc::new(Bash::default()))]
}

pub fn build(card: &AgentCard, card_hash: &str) -> Result<AgentRuntime, FactoryError> {
    factory::build_with(card, card_hash, &tool_registry())
}

#[cfg(test)]
mod tests {
    use super::*;
    use an_agent_core::act::ToolTag;
    use an_agent_core::principal::card::{ModelSpec, ToolGrant, Topology};
    use uuid::Uuid;

    fn card() -> AgentCard {
        AgentCard {
            v: 1,
            id: Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap(),
            model: ModelSpec {
                base_url: "https://example.com".into(),
                model: "m".into(),
                extra_body: None,
            },
            prompt: "test prompt".into(),
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
    fn build_registers_bash() {
        let rt = build(&card(), "hash-1").unwrap();
        assert_eq!(rt.prompt, "test prompt");
        assert_eq!(rt.tools.len(), 1);
        assert_eq!(rt.tools[0].name(), "bash");
        assert_eq!(rt.tools[0].tag_seed(), Some(ToolTag::none()));
        assert_eq!(rt.card_hash, "hash-1");
    }
}
