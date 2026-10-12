//! The tape adapter for the independent semantic plane. Domain boundaries
//! construct attempts and claims; this adapter records every decision before
//! execution and returns a locally issued, exactly bound grant.

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
    ) -> Result<AuthorizedAttempt, ControlError> {
        let decision = decide(attempt, claims);
        let tape_id = self.append_event(
            agent,
            Kind::Action,
            "semantic",
            json!({ "attempt": attempt, "claims": claims, "decision": decision }).to_string(),
            attempt.refs.clone(),
        )?;
        if decision.disposition == Disposition::Allow
            && let Some(grant) = decision.grant.filter(|grant| grant.matches(attempt))
        {
            return Ok(AuthorizedAttempt { grant, tape_id });
        }
        Err(ControlError::SemanticDenied {
            operation: attempt.capability.operation.clone(),
            subject: attempt.subject.clone(),
            reason: decision
                .reason
                .map(|reason| reason.as_str())
                .unwrap_or("unbound_grant")
                .into(),
        })
    }
}
