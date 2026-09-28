//! Live agent: card-derived identity plus the current private session.

use std::sync::Arc;

use thiserror::Error;
use uuid::Uuid;

use crate::act::{ActCtx, Tool};
use crate::principal::factory::AgentRuntime;

use super::session::Session;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("agent id is not a UUID: {0}")]
    AgentId(String),
    #[error("session belongs to {found}, not {expected}")]
    SessionMismatch { expected: Uuid, found: Uuid },
}

/// Scheduled unit for the loop. Workplace mount is out of scope.
pub struct Agent {
    id: Uuid,
    id_str: String,
    card_hash: String,
    prompt: String,
    tools: Vec<Arc<dyn Tool>>,
    session: Session,
}

impl Agent {
    pub fn from_runtime(rt: AgentRuntime, session: Session) -> Result<Self, AgentError> {
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
