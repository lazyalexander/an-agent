//! Delivery currentness and host landing evidence.
use super::{AgentControl, ControlError, DeliveryOutcome, SpoolDeclaration, SpoolReply};
use crate::agent::Agent;
use an_agent_semantic::FactRef;
use an_agent_supervision::Supervision;
use serde_json::json;
use uuid::Uuid;

impl AgentControl {
    /// The host landed `reply` in software. Core writes that claim on the
    /// thread tape and cites the spool note. It does not mutate the host.
    /// Does not take the beat slot. Stale claims are audited as applied_stale
    /// and return an error; this neither authorizes nor undoes an external effect.
    pub fn applied(
        &self,
        id: Uuid,
        reply: &SpoolReply,
        note: &str,
    ) -> Result<String, ControlError> {
        let agent = self.live(id)?;
        let state = self
            .inner
            .supervision
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let current = self.check_delivery_current(&state, &agent, &reply.delivery_facts);
        let content = json!({
            "spool": reply.spool,
            "event": reply.event_id,
            "reply": reply.reply,
            "note": note,
            "delivery_facts": reply.delivery_facts,
            "current": current.is_ok(),
            "stale_reason": current.as_ref().err().map(ToString::to_string),
        })
        .to_string();
        let refs = if reply.tape_id.is_empty() {
            Vec::new()
        } else {
            vec![reply.tape_id.clone()]
        };
        if let Err(error) = current {
            self.append_audit_event(
                &agent,
                crate::memstream::Kind::Action,
                "applied_stale",
                content,
                refs,
            )?;
            return Err(error);
        }
        self.append_event(
            &agent,
            crate::memstream::Kind::Action,
            "applied",
            content,
            refs,
        )
    }

    /// Check immediately before executing a reply's next intent. This is not a
    /// transaction with external software; report any completed effect via applied.
    pub fn validate_reply(&self, id: Uuid, reply: &SpoolReply) -> Result<(), ControlError> {
        let agent = self.live(id)?;
        self.ensure_delivery_current(&agent, &reply.delivery_facts)
    }

    pub(super) fn ensure_delivery_current(
        &self,
        agent: &Agent,
        facts: &[FactRef],
    ) -> Result<(), ControlError> {
        self.with_delivery_current(agent, facts, || Ok(()))
    }

    pub(super) fn with_delivery_current<T>(
        &self,
        agent: &Agent,
        facts: &[FactRef],
        publish: impl FnOnce() -> Result<T, ControlError>,
    ) -> Result<T, ControlError> {
        let state = self
            .inner
            .supervision
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Err(error) = self.check_delivery_current(&state, agent, facts) {
            self.append_audit_event(
                agent,
                crate::memstream::Kind::Action,
                "delivery_stale",
                json!({"facts":facts,"reason":error.to_string()}).to_string(),
                Vec::new(),
            )?;
            return Err(error);
        }
        // Same lock as register/unmount/release; no user callback or external-application I/O.
        publish()
    }

    pub(super) fn publish_reply(
        &self,
        agent: &Agent,
        name: &str,
        event_id: &str,
        declaration: &SpoolDeclaration,
        reply: String,
        facts: &[FactRef],
    ) -> Result<DeliveryOutcome, ControlError> {
        self.with_delivery_current(agent, facts, || {
            let tape_id = self.note_spool(agent, name, event_id, Ok(reply.as_str()))?;
            let intents = self.admit_intents(
                agent,
                name,
                event_id,
                declaration,
                super::spool::parse_intents(&reply),
            )?;
            Ok(DeliveryOutcome::Reply(SpoolReply {
                spool: name.to_owned(),
                event_id: event_id.to_owned(),
                reply,
                tape_id,
                intents,
                delivery_facts: facts.to_vec(),
            }))
        })
    }

    fn check_delivery_current(
        &self,
        state: &Supervision,
        agent: &Agent,
        facts: &[FactRef],
    ) -> Result<(), ControlError> {
        let registration = self.current_registration_snapshot()?;
        for (kind, revision) in [
            ("workspace.registration", registration.revision),
            ("workspace.registration.generation", registration.generation),
        ] {
            let mut matching = facts.iter().filter(|fact| fact.kind == kind);
            if matching.next().map(|fact| &fact.revision) != Some(&revision)
                || matching.next().is_some()
            {
                return Err(ControlError::ReviewStale(kind.into()));
            }
        }
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
