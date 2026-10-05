//! Mounted bodies and delivery. A body receives one workspace event.
//! It does not read the log, write the tape, or store config or env.

use std::sync::Arc;

use serde_json::json;
use uuid::Uuid;

use super::{AgentControl, ControlError};
use crate::agent::Agent;
use crate::workspace::WorkspaceRecord;

/// A body that receives one workspace event from [`AgentControl::offer`]
/// or [`AgentControl::dispatch`]. It does not read the log, write the tape,
/// or call another body.
pub trait SpoolBeat: Send + Sync {
    fn receive(&self, event: &WorkspaceRecord) -> Result<String, String>;
}

/// What one offered body returned. The workspace event was already on the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpoolReply {
    pub spool: String,
    pub event_id: String,
    pub reply: String,
}

impl AgentControl {
    /// Mount a body under `name`. A second mount of the same name is refused.
    pub fn mount_spool(&self, name: &str, body: Arc<dyn SpoolBeat>) -> Result<(), ControlError> {
        let mut bodies = self
            .inner
            .bodies
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        if bodies.iter().any(|(mounted, _)| mounted == name) {
            return Err(ControlError::AlreadyMounted(name.to_string()));
        }
        bodies.push((name.to_string(), body));
        Ok(())
    }

    /// Remove a mounted body. The workspace log is left as it is.
    pub fn unmount_spool(&self, name: &str) -> Result<(), ControlError> {
        let mut bodies = self
            .inner
            .bodies
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let Some(pos) = bodies.iter().position(|(mounted, _)| mounted == name) else {
            return Err(ControlError::NotMounted(name.to_string()));
        };
        bodies.remove(pos);
        Ok(())
    }

    /// One beat. The named body receives the latest workspace event.
    /// Core writes the reply on the thread tape. The body does not.
    /// A failure unmounts that body only. The event stays on the log.
    pub fn offer(&self, id: Uuid, name: &str) -> Result<SpoolReply, ControlError> {
        self.enter_beat(id)?;
        let turn = self.inner.pool.begin_turn(id)?;
        let event = self.latest_event()?;
        let Some(body) = self.body_named(name) else {
            return Err(ControlError::NotMounted(name.to_string()));
        };
        match body.receive(&event) {
            Ok(reply) => {
                self.note_spool(turn.agent(), name, &event.id, Ok(reply.as_str()))?;
                Ok(SpoolReply {
                    spool: name.to_string(),
                    event_id: event.id,
                    reply,
                })
            }
            Err(reason) => {
                self.drop_body(name);
                self.note_spool(turn.agent(), name, &event.id, Err(reason.as_str()))?;
                Err(ControlError::SpoolFailed {
                    name: name.to_string(),
                    reason,
                })
            }
        }
    }

    /// One beat. The latest event goes to each spool on its route, in order.
    /// A missing spool is noted and skipped. A failed spool is unmounted and
    /// the rest still run. Replies are returned to the host. They are not
    /// written as new workspace events.
    pub fn dispatch(&self, id: Uuid) -> Result<Vec<SpoolReply>, ControlError> {
        self.enter_beat(id)?;
        let turn = self.inner.pool.begin_turn(id)?;
        let event = self.latest_event()?;
        let event_name = event.name.clone().unwrap_or_default();
        let route = self.spools_for(&event_name)?;
        let mut replies = Vec::new();
        for spool in route {
            let Some(body) = self.body_named(&spool) else {
                self.note_spool(turn.agent(), &spool, &event.id, Err("missing"))?;
                continue;
            };
            match body.receive(&event) {
                Ok(reply) => {
                    self.note_spool(turn.agent(), &spool, &event.id, Ok(reply.as_str()))?;
                    replies.push(SpoolReply {
                        spool,
                        event_id: event.id.clone(),
                        reply,
                    });
                }
                Err(reason) => {
                    self.drop_body(&spool);
                    self.note_spool(turn.agent(), &spool, &event.id, Err(reason.as_str()))?;
                }
            }
        }
        drop(turn);
        Ok(replies)
    }
    fn body_named(&self, name: &str) -> Option<Arc<dyn SpoolBeat>> {
        let bodies = self
            .inner
            .bodies
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        bodies
            .iter()
            .find(|(mounted, _)| mounted == name)
            .map(|(_, body)| Arc::clone(body))
    }

    fn drop_body(&self, name: &str) {
        let mut bodies = self
            .inner
            .bodies
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        bodies.retain(|(mounted, _)| mounted != name);
    }

    fn note_spool(
        &self,
        agent: &Agent,
        spool: &str,
        event_id: &str,
        outcome: Result<&str, &str>,
    ) -> Result<(), ControlError> {
        let content = match outcome {
            Ok(reply) => json!({ "spool": spool, "event": event_id, "reply": reply }),
            Err(reason) => json!({ "spool": spool, "event": event_id, "error": reason }),
        };
        self.append(agent, "spool", content.to_string())?;
        Ok(())
    }
    fn latest_event(&self) -> Result<WorkspaceRecord, ControlError> {
        self.inner
            .workspace
            .log()?
            .into_iter()
            .rev()
            .find(|record| record.kind == "event")
            .ok_or(ControlError::NoEvent)
    }
}
