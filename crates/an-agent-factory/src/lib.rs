//! Bash is the one constructor this crate registers. The kernel builds
//! grants from whatever list the caller passes. `spawn` opens a session
//! with that list. Worker seats stay on the runtime.

mod bash;

pub use bash::Bash;

use std::path::Path;
use std::sync::Arc;

use an_agent_core::principal::card::AgentCard;
use an_agent_core::principal::factory::{self, BuiltAgent, FactoryError, ToolCtor};
use an_agent_core::runtime::{Agent, RuntimeError};

pub fn tool_registry() -> Vec<(&'static str, ToolCtor)> {
    vec![("bash", || Arc::new(Bash::default()))]
}

pub fn build(card: &AgentCard, card_hash: &str) -> Result<BuiltAgent, FactoryError> {
    factory::build_with(card, card_hash, &tool_registry())
}

pub fn spawn(card: &AgentCard, sessions_root: impl AsRef<Path>) -> Result<Agent, RuntimeError> {
    an_agent_core::runtime::spawn_with(card, sessions_root, &tool_registry())
}

pub fn spawn_arc(
    card: &AgentCard,
    sessions_root: impl AsRef<Path>,
) -> Result<Arc<Agent>, RuntimeError> {
    Ok(Arc::new(spawn(card, sessions_root)?))
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
