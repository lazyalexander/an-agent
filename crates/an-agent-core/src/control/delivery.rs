//! Delivery currentness and host landing evidence.
use super::{AgentControl, ControlError, SpoolReply};
use crate::agent::Agent;
use an_agent_semantic::FactRef;
use serde_json::json;
use uuid::Uuid;

impl AgentControl {
    /// The host landed `reply` in software. Core writes that claim on the
    /// thread tape and cites the spool note. It does not mutate the host.
    /// Does not take the beat slot.
    pub fn applied(
        &self,
        id: Uuid,
        reply: &SpoolReply,
        note: &str,
    ) -> Result<String, ControlError> {
        let agent = self.live(id)?;
        let content = json!({
            "spool": reply.spool,
            "event": reply.event_id,
            "reply": reply.reply,
            "note": note,
        })
        .to_string();
        let refs = if reply.tape_id.is_empty() {
            Vec::new()
        } else {
            vec![reply.tape_id.clone()]
        };
        self.append_event(
            &agent,
            crate::memstream::Kind::Action,
            "applied",
            content,
            refs,
        )
    }

    pub(super) fn ensure_delivery_current(
        &self,
        agent: &Agent,
        facts: &[FactRef],
    ) -> Result<(), ControlError> {
        let state = self
            .inner
            .supervision
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let current = state.instance(agent.id())?;
        let instance = current.incarnation.to_string();
        if !facts
            .iter()
            .any(|fact| fact.kind == "agent.instance" && fact.revision == instance)
        {
            return Err(ControlError::ReviewStale(instance));
        }
        let mount = facts
            .iter()
            .find(|fact| fact.kind == "spool.instance")
            .and_then(|fact| Uuid::parse_str(&fact.revision).ok());
        if !mount.is_some_and(|id| state.mounted(id)) {
            return Err(ControlError::ReviewStale("retired mount".into()));
        }
        Ok(())
    }
}
