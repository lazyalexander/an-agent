//! Card-driven construction of a granted tool set: a static constructor
//! registry plus `build`. Thin by design — no dynamic registration
//! machinery. Memory, model clients, and the loop stay with the caller;
//! the card never holds keys or runtime deps.

use std::sync::Arc;

use thiserror::Error;

use crate::act::{FileFacet, Tool, ToolCtx, ToolTag};
use crate::tools::Bash;

use super::card::AgentCard;

#[derive(Debug, Error)]
pub enum FactoryError {
    #[error("unknown tool in grants: {0}")]
    UnknownTool(String),
    // act cannot resolve workplace resources yet (ActCtx carries none), so
    // a file-faced grant would die at run time with MissingResource. Fail
    // at build instead of implying the tool is runnable.
    #[error("grant for {0} declares a file face the runtime cannot wire yet")]
    UnsupportedFace(String),
}

/// What a card builds: identity, prompt, and the granted tool set. Model
/// clients and message state belong to the caller's chosen loop (today: the
/// probe in tests/), not to the factory.
pub struct AgentRuntime {
    pub card_hash: String,
    pub agent_id: String,
    pub prompt: String,
    pub tools: Vec<Arc<dyn Tool>>,
}

/// Name → constructor. Static and thin on purpose.
pub type ToolCtor = fn() -> Arc<dyn Tool>;

pub fn tool_registry() -> Vec<(&'static str, ToolCtor)> {
    vec![("bash", || Arc::new(Bash::default()))]
}

/// Wraps a constructed tool so the grant's tag wins over the constructor's
/// default tag_seed: permissions come from the card.
struct Granted {
    inner: Arc<dyn Tool>,
    tag: ToolTag,
}

#[async_trait::async_trait]
impl Tool for Granted {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn description(&self) -> &str {
        self.inner.description()
    }
    fn parameters(&self) -> serde_json::Value {
        self.inner.parameters()
    }
    fn tag_seed(&self) -> Option<ToolTag> {
        Some(self.tag.clone())
    }
    async fn execute(&self, args: serde_json::Value, ctx: &ToolCtx) -> Result<String, String> {
        self.inner.execute(args, ctx).await
    }
}

pub fn build(card: &AgentCard, card_hash: &str) -> Result<AgentRuntime, FactoryError> {
    let registry = tool_registry();
    let mut tools: Vec<Arc<dyn Tool>> = Vec::new();
    for grant in &card.tools {
        let (_, ctor) = registry
            .iter()
            .find(|(name, _)| *name == grant.name)
            .ok_or_else(|| FactoryError::UnknownTool(grant.name.clone()))?;
        if matches!(
            grant.tag.file,
            FileFacet::Read { .. } | FileFacet::Write { .. } | FileFacet::ReadWrite { .. }
        ) {
            return Err(FactoryError::UnsupportedFace(grant.name.clone()));
        }
        tools.push(Arc::new(Granted {
            inner: ctor(),
            tag: grant.tag.clone(),
        }));
    }
    Ok(AgentRuntime {
        card_hash: card_hash.to_string(),
        agent_id: card.id.to_string(),
        prompt: card.prompt.clone(),
        tools,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::act::{ActCtx, Permit, ToolCall, run_tool_act};
    use crate::memstream::{JsonlStore, Kind};
    use crate::principal::card::{ModelSpec, ToolGrant, Topology};
    use crate::testkit::TempDir;
    use uuid::Uuid;

    fn ctx() -> ToolCtx {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        ToolCtx { signal: Some(rx) }
    }

    fn card(tag: ToolTag) -> AgentCard {
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
                tag,
            }],
            topology: Topology::Leaf,
            kernel: "0.1.0".into(),
            supersedes: None,
        }
    }

    #[test]
    fn build_primes_prompt_and_grants() {
        let c = card(ToolTag::none());
        let rt = build(&c, "hash-1").unwrap();
        assert_eq!(rt.prompt, "test prompt");
        assert_eq!(rt.tools.len(), 1);
        assert_eq!(rt.tools[0].name(), "bash");
        assert_eq!(rt.tools[0].tag_seed(), Some(ToolTag::none()));
        assert_eq!(rt.card_hash, "hash-1");
    }

    #[test]
    fn unknown_tool_name_fails_build() {
        let mut c = card(ToolTag::none());
        c.tools[0].name = "nope".into();
        assert!(matches!(build(&c, "h"), Err(FactoryError::UnknownTool(_))));
    }

    #[test]
    fn file_faced_grant_is_rejected_until_act_wires_resources() {
        let c = card(ToolTag::read("/x"));
        assert!(
            matches!(build(&c, "h"), Err(FactoryError::UnsupportedFace(name)) if name == "bash")
        );
    }

    #[tokio::test]
    async fn forbidden_grant_not_executed_and_tape_cites_card() {
        let c = card(ToolTag::none_permit(Permit::Forbidden));
        let rt = build(&c, "hash-x").unwrap();
        let tmp = TempDir::new("factory");
        let store = JsonlStore::open(tmp.path().join("memory.jsonl")).unwrap();
        let actx = ActCtx {
            store: Some(&store),
            agent_id: &rt.agent_id,
            session: "s1",
            card: Some(&rt.card_hash),
        };
        let call = ToolCall {
            id: "c1".into(),
            name: "bash".into(),
            arguments: r#"{"command":"true"}"#.into(),
        };
        let result = run_tool_act(&actx, &rt.tools, &call, &ctx()).await.unwrap();
        assert_eq!(result.message.content, "forbidden");
        let events = store.read_all().unwrap();
        let obs = events.iter().find(|e| e.kind == Kind::Observation).unwrap();
        assert_eq!(obs.content, "forbidden");
        assert_eq!(obs.card.as_deref(), Some("hash-x"));
    }
}
