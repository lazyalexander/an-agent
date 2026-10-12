//! The tape adapter for the independent semantic plane. Domain boundaries
//! construct attempts and claims; this adapter records every decision before
//! execution. The control boundary, not this adapter, owns stopping an
//! operation and asking its supervisor for a disposition.

use an_agent_semantic::{Attempt, Claim, Disposition, FactRef, Grant, decide};
use serde::Serialize;
use serde_json::json;

use super::{AgentControl, ControlError};
use crate::agent::Agent;
use crate::memstream::Kind;

pub(super) struct AuthorizedAttempt {
    pub grant: Grant,
    pub tape_id: String,
}

pub(super) enum Admission {
    Allowed(AuthorizedAttempt),
    Denied {
        attempt: Attempt,
        tape_id: String,
        reason: String,
    },
}

impl AgentControl {
    /// Keep the bytes behind each local fact address, not just its digest.
    pub(super) fn semantic_snapshot(
        &self,
        agent: &Agent,
        kind: &str,
        value: &impl Serialize,
    ) -> Result<FactRef, ControlError> {
        let bytes = serde_json::to_vec(value)
            .map_err(|err| ControlError::SemanticRecord(err.to_string()))?;
        let sha = agent.session().put_blob(&bytes)?;
        Ok(FactRef::new(kind, sha))
    }

    pub(super) fn authorize_attempt(
        &self,
        agent: &Agent,
        attempt: &Attempt,
        claims: &[Claim],
    ) -> Result<Admission, ControlError> {
        let decision = decide(attempt, claims);
        let tape_id = self.append_audit_event(
            agent,
            Kind::Action,
            "semantic",
            json!({ "attempt": attempt, "claims": claims, "decision": decision }).to_string(),
            attempt.refs.clone(),
        )?;
        if decision.disposition == Disposition::Allow
            && let Some(grant) = decision.grant.filter(|grant| grant.matches(attempt))
        {
            return Ok(Admission::Allowed(AuthorizedAttempt { grant, tape_id }));
        }
        Ok(Admission::Denied {
            attempt: attempt.clone(),
            tape_id,
            reason: decision
                .reason
                .map(|reason| reason.as_str())
                .unwrap_or("unbound_grant")
                .into(),
        })
    }
}
