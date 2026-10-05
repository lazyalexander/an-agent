//! Live agent: card-derived identity plus its private session directory.
//! `spawn_with` binds a built card to a fresh directory and its tape.
//! It is not the host face.

mod session;

use std::path::Path;
use std::sync::Arc;

use thiserror::Error;
use uuid::Uuid;

use crate::act::{ActCtx, Tool};
use crate::principal::card::AgentCard;
use crate::principal::factory::{self, BuiltAgent, FactoryError};

pub use session::{Session, SessionError, SessionManifest};

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("agent id is not a UUID: {0}")]
    AgentId(String),
    #[error("session belongs to {found}, not {expected}")]
    SessionMismatch { expected: Uuid, found: Uuid },
}

/// Scheduled unit for the loop. Workspace mount is out of scope.
pub struct Agent {
    id: Uuid,
    id_str: String,
    card_hash: String,
    prompt: String,
    tools: Vec<Arc<dyn Tool>>,
    session: Session,
}

impl Agent {
    pub fn from_built(rt: BuiltAgent, session: Session) -> Result<Self, AgentError> {
        let id: Uuid = rt
            .agent_id
            .parse()
            .map_err(|_| AgentError::AgentId(rt.agent_id.clone()))?;
        if session.agent_id() != id {
            return Err(AgentError::SessionMismatch {
                expected: id,
                found: session.agent_id(),
            });
        }
        Ok(Self {
            id,
            id_str: rt.agent_id,
            card_hash: rt.card_hash,
            prompt: rt.prompt,
            tools: rt.tools,
            session,
        })
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn id_str(&self) -> &str {
        &self.id_str
    }

    pub fn card_hash(&self) -> &str {
        &self.card_hash
    }

    pub fn prompt(&self) -> &str {
        &self.prompt
    }

    pub fn tools(&self) -> &[Arc<dyn Tool>] {
        &self.tools
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn act_ctx(&self) -> ActCtx<'_> {
        ActCtx {
            store: Some(self.session.tape()),
            agent_id: &self.id_str,
            session: self.session.id_str(),
            card: Some(&self.card_hash),
        }
    }
}

#[derive(Debug, Error)]
pub enum SpawnError {
    #[error(transparent)]
    Factory(#[from] FactoryError),
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error(transparent)]
    Agent(#[from] AgentError),
    #[error("card hash: {0}")]
    CardHash(#[from] serde_json::Error),
}

/// Card, then a fresh private directory and tape. The kernel registers no tools.
pub fn spawn_with(
    card: &AgentCard,
    sessions_root: impl AsRef<Path>,
    registry: &[(&str, factory::ToolCtor)],
) -> Result<Agent, SpawnError> {
    let hash = card.hash()?;
    let built = factory::build_with(card, &hash, registry)?;
    let session = Session::create(sessions_root, card.id)?;
    Ok(Agent::from_built(built, session)?)
}

/// Bind a card to an existing session directory. Does not create a tape.
pub fn spawn_resume(
    card: &AgentCard,
    sessions_root: impl AsRef<Path>,
    session_id: Uuid,
    registry: &[(&str, factory::ToolCtor)],
) -> Result<Agent, SpawnError> {
    let hash = card.hash()?;
    let built = factory::build_with(card, &hash, registry)?;
    let session = Session::open(sessions_root, card.id, session_id)?;
    Ok(Agent::from_built(built, session)?)
}

pub fn spawn_arc_with(
    card: &AgentCard,
    sessions_root: impl AsRef<Path>,
    registry: &[(&str, factory::ToolCtor)],
) -> Result<Arc<Agent>, SpawnError> {
    Ok(Arc::new(spawn_with(card, sessions_root, registry)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::act::{Permit, ToolCall, ToolCtx, ToolTag, run_tool_act};
    use crate::principal::card::{ModelSpec, ToolGrant, Topology};
    use crate::testkit::{TempDir, bash_registry};
    use uuid::Uuid;

    fn card(id: &str) -> AgentCard {
        AgentCard {
            v: 1,
            id: Uuid::parse_str(id).unwrap(),
            model: ModelSpec {
                base_url: "https://example.com".into(),
                model: "m".into(),
                extra_body: None,
            },
            prompt: "p".into(),
            tools: vec![ToolGrant {
                name: "bash".into(),
                tag: ToolTag::none_permit(Permit::Deny),
            }],
            topology: Topology::Leaf,
            kernel: "0.1.0".into(),
            supersedes: None,
        }
    }

    fn ctx() -> ToolCtx {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        ToolCtx { signal: Some(rx) }
    }

    #[test]
    fn spawn_binds_private_session_and_act_ctx() {
        let tmp = TempDir::new("instance-spawn");
        let agent = spawn_with(
            &card("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"),
            tmp.path(),
            &bash_registry(),
        )
        .unwrap();
        assert!(agent.session().files().is_dir());
        let actx = agent.act_ctx();
        assert_eq!(actx.agent_id, agent.id_str());
        assert_eq!(actx.session, agent.session().id_str());
        assert_eq!(actx.card, Some(agent.card_hash()));
        assert!(actx.store.is_some());
    }

    #[tokio::test]
    async fn spawned_agent_can_admit_through_session_tape() {
        let tmp = TempDir::new("instance-act");
        let agent = spawn_with(
            &card("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"),
            tmp.path(),
            &bash_registry(),
        )
        .unwrap();
        let call = ToolCall {
            id: "c1".into(),
            name: "bash".into(),
            arguments: r#"{"command":"true"}"#.into(),
        };
        let result = run_tool_act(&agent.act_ctx(), agent.tools(), &call, &ctx())
            .await
            .unwrap();
        assert_eq!(result.message.content, "deny");
        assert_eq!(agent.session().tape().read_all().unwrap().len(), 2);
    }
}
