//! Trusted boundary adapter for the independent supervision state machine.
//! Core owns authentication, resource ceilings, evidence, mail and scheduling.

use an_agent_semantic::{Attempt, Capability, FactRef, Polarity, Resource};
use serde_json::json;
use uuid::Uuid;

use super::review_state::{CallBinding, ReviewError, Supervision};
pub use super::review_state::{
    InstanceRef, ReviewCause, ReviewRequest, ReviewResolution, ReviewScope,
};
use super::{AgentControl, ControlError};
use crate::act::{Charter, FileFacet, Permit, ToolCall, tag_of};
use crate::agent::Agent;
use crate::memstream::Kind;

impl From<ReviewError> for ControlError {
    fn from(error: ReviewError) -> Self {
        match error {
            ReviewError::Binding(reason) => Self::SupervisorBinding(reason),
            ReviewError::Inactive(id) => Self::NotOpen(id),
            ReviewError::Missing(id) => Self::ReviewMissing(id),
            ReviewError::Authority(id) => Self::ReviewAuthority(id),
            ReviewError::Stale(id) => Self::ReviewStale(id),
            ReviewError::Ceiling(id) => Self::ReviewCeiling(id),
        }
    }
}

impl AgentControl {
    /// Trusted host wiring between live instances, not a capability grant.
    pub fn set_supervisor(&self, child: Uuid, supervisor: Uuid) -> Result<(), ControlError> {
        let mut state = self
            .inner
            .supervision
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let agent = self.live(child)?;
        self.live(supervisor)?;
        state.check_binding(child, supervisor)?;
        self.append_audit_event(
            &agent, Kind::Observation, "supervisor",
            json!({"supervisor": supervisor, "child_instance": state.instance(child)?, "supervisor_instance": state.instance(supervisor)?}).to_string(),
            vec![],
        )?;
        state.bind(child, supervisor)?;
        Ok(())
    }

    /// Return the live supervisor's inbox. Reading it never schedules a beat.
    pub fn pending_reviews(&self, supervisor: Uuid) -> Result<Vec<ReviewRequest>, ControlError> {
        let state = self
            .inner
            .supervision
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        self.live(supervisor)?;
        state.instance(supervisor)?;
        Ok(state.pending(supervisor))
    }

    /// Revoke the live instance's review authority and all reviews involving
    /// it. The binding remains, but future requests use a fresh authority epoch.
    pub fn revoke_reviews(&self, principal: Uuid, reason: &str) -> Result<(), ControlError> {
        let mut state = self
            .inner
            .supervision
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        self.live(principal)?;
        let requests = state.revoke(principal)?;
        self.record_review_closures(requests, reason)
    }

    /// The host authenticates the caller. Advice and responses never run a
    /// beat or widen a resource ceiling. Revocation precedes fallible mail.
    pub fn resolve_review(
        &self,
        supervisor: Uuid,
        request_id: &str,
        resolution: ReviewResolution,
    ) -> Result<(), ControlError> {
        let mut state = self
            .inner
            .supervision
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let approver = self.live(supervisor)?;
        state.check_resolution(supervisor, request_id, &resolution)?;
        let pending = state.get(request_id)?.clone();
        if !self.review_facts_current(&pending.request.attempt.facts)? {
            self.close_review(&mut state, request_id, "stale")?;
            return Err(ControlError::ReviewStale(request_id.into()));
        }
        let child = self.live(pending.request.child)?;
        let approving = matches!(resolution, ReviewResolution::ApproveOnce { .. });
        if approving {
            let call = pending
                .call
                .as_ref()
                .ok_or_else(|| ControlError::ReviewCeiling(request_id.into()))?;
            self.check_review_ceiling(&child, &approver, &call.operation, request_id)?;
        } else {
            state.resolve(supervisor, request_id, &resolution, approver.card_hash())?;
        }
        let message =
            json!({"review": request_id, "resolution": resolution, "scope": pending.request.scope})
                .to_string();
        self.append_audit_event(
            &child,
            Kind::Observation,
            "review_resolution",
            message.clone(),
            vec![pending.request.tape_id.clone()],
        )?;
        self.send(supervisor, child.id(), &message)?;
        if approving {
            state.resolve(supervisor, request_id, &resolution, approver.card_hash())?;
        }
        Ok(())
    }

    fn check_review_ceiling(
        &self,
        child: &Agent,
        approver: &Agent,
        operation: &str,
        request: &str,
    ) -> Result<(), ControlError> {
        let child_tool = child
            .tools()
            .iter()
            .find(|tool| tool.name() == operation)
            .ok_or_else(|| ControlError::ReviewCeiling(request.into()))?;
        let approver_tool = approver
            .tools()
            .iter()
            .find(|tool| tool.name() == operation)
            .ok_or_else(|| ControlError::ReviewCeiling(request.into()))?;
        let child_tag = tag_of(Some(child_tool.as_ref()));
        let approver_tag = tag_of(Some(approver_tool.as_ref()));
        let file_fits = child_tag.file == approver_tag.file
            || matches!(child_tag.file, FileFacet::None)
            || matches!(approver_tag.file, FileFacet::Unbounded);
        if child_tag.permit != Permit::Ask
            || approver_tag.permit != Permit::Go
            || !file_fits
            || !Charter::new(approver_tag).allows(&child_tag)
        {
            return Err(ControlError::ReviewCeiling(request.into()));
        }
        Ok(())
    }

    pub(super) fn waiting_review(
        &self,
        child: Uuid,
        subject: &str,
        mount: Uuid,
    ) -> Result<Option<ReviewRequest>, ControlError> {
        let state = self
            .inner
            .supervision
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let scope = state.scope(child, Some(mount))?;
        Ok(state
            .waiting(&scope, subject, false)
            .map(|p| p.request.clone()))
    }

    pub(super) fn request_review(
        &self,
        agent: &Agent,
        subject: &str,
        event_id: Option<&str>,
        mut attempt: Attempt,
        cause: ReviewCause,
        call: Option<ToolCall>,
    ) -> Result<ReviewRequest, ControlError> {
        let mut state = self
            .inner
            .supervision
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        for fact in self.review_generations()? {
            if !attempt
                .facts
                .iter()
                .any(|existing| existing.kind == fact.kind)
            {
                attempt.facts.push(fact);
            }
        }
        let binding = call.as_ref().map(call_binding).transpose()?;
        self.retain_review(
            &mut state, agent, subject, event_id, attempt, cause, binding,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn retain_review(
        &self,
        state: &mut Supervision,
        agent: &Agent,
        subject: &str,
        event_id: Option<&str>,
        attempt: Attempt,
        cause: ReviewCause,
        call: Option<CallBinding>,
    ) -> Result<ReviewRequest, ControlError> {
        let mount = attempt
            .facts
            .iter()
            .find(|fact| fact.kind == "spool.instance")
            .map(|fact| {
                Uuid::parse_str(&fact.revision)
                    .map_err(|e| ControlError::SemanticRecord(e.to_string()))
            })
            .transpose()?;
        let scope = state.scope(agent.id(), mount)?;
        if !self.review_facts_current(&attempt.facts)? {
            return Err(ControlError::ReviewStale(attempt.id));
        }
        if let Some(pending) = state.waiting(&scope, subject, call.is_some()) {
            return Ok(pending.request.clone());
        }
        let mut request = ReviewRequest {
            id: crate::det_seam::Entropy::os().uuid_v4().to_string(),
            child: agent.id(),
            supervisor: scope.supervisor.map(|s| s.principal),
            subject: subject.into(),
            event_id: event_id.map(str::to_string),
            attempt,
            cause,
            tape_id: String::new(),
            scope: Some(scope),
        };
        request.tape_id = self.append_audit_event(
            agent,
            Kind::Utterance,
            "review_request",
            serde_json::to_string(&request)
                .map_err(|e| ControlError::SemanticRecord(e.to_string()))?,
            request.attempt.refs.clone(),
        )?;
        if let Some(supervisor) = request.supervisor {
            self.send(
                agent.id(),
                supervisor,
                &serde_json::to_string(&request)
                    .map_err(|e| ControlError::SemanticRecord(e.to_string()))?,
            )?;
            state.insert(request.clone(), call)?;
        }
        // An unassigned request returns to the host, without issuing authority.
        Ok(request)
    }

    /// Revocations are already applied. Attempt all recipients even if one
    /// tape fails; evidence failure must not revive any removed approval.
    pub(super) fn record_review_closures(
        &self,
        requests: Vec<ReviewRequest>,
        reason: &str,
    ) -> Result<(), ControlError> {
        let mut failure = None;
        for request in requests {
            for id in std::iter::once(request.child).chain(request.supervisor) {
                let Ok(agent) = self.mounted(id) else {
                    continue;
                };
                let result = self.append_audit_event(
                    &agent,
                    Kind::Observation,
                    "review_closed",
                    json!({"review": request.id, "reason": reason, "scope": request.scope})
                        .to_string(),
                    vec![request.tape_id.clone()],
                );
                if let Err(error) = result
                    && failure.is_none()
                {
                    failure = Some(error);
                }
            }
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn close_review(
        &self,
        state: &mut Supervision,
        id: &str,
        reason: &str,
    ) -> Result<(), ControlError> {
        self.record_review_closures(state.close(id).into_iter().collect(), reason)
    }

    fn review_generations(&self) -> Result<Vec<FactRef>, ControlError> {
        let cite = self.workspace_cite()?;
        Ok([
            ("workspace.registration.generation", cite.register_id),
            ("workspace.config.generation", cite.config_id),
            ("workspace.env.generation", cite.env_id),
        ]
        .into_iter()
        .map(|(kind, revision)| FactRef::new(kind, revision.as_deref().unwrap_or("absent")))
        .collect())
    }

    fn review_facts_current(&self, facts: &[FactRef]) -> Result<bool, ControlError> {
        Ok(self.review_generations()?.iter().all(|current| {
            facts
                .iter()
                .filter(|fact| fact.kind == current.kind)
                .all(|fact| fact == current)
        }))
    }

    pub(super) fn review_tool(&self, agent: &Agent, call: &ToolCall) -> Result<bool, ControlError> {
        let mut state = self
            .inner
            .supervision
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let scope = state.scope(agent.id(), None)?;
        let tool = agent.tools().iter().find(|tool| tool.name() == call.name);
        let tag = tag_of(tool.map(|tool| tool.as_ref()));
        if tool.is_some() && tag.permit == Permit::Go {
            return Ok(false);
        }
        let subject = format!("tool:{}", call.name);
        let binding = call_binding(call)?;
        let mut facts = vec![
            FactRef::new("agent.card", agent.card_hash()),
            FactRef::new("agent.instance", scope.child.incarnation.to_string()),
            self.semantic_snapshot(agent, "tool.request", call)?,
        ];
        let cite = self.workspace_cite()?;
        for (kind, revision) in [
            ("workspace.registration", cite.register_sha256),
            ("workspace.config", cite.config_sha256),
            ("workspace.env", cite.env_sha256),
        ] {
            if let Some(revision) = revision {
                facts.push(FactRef::new(kind, revision));
            }
        }
        facts.extend(self.review_generations()?);
        if let Some(pending) = state.waiting(&scope, &subject, true).cloned() {
            let id = &pending.request.id;
            let approver = self.live(
                pending
                    .request
                    .supervisor
                    .ok_or_else(|| ControlError::ReviewAuthority(id.clone()))?,
            )?;
            match state.check_call(id, &binding, &facts, approver.card_hash()) {
                Err(ReviewError::Stale(_)) => {
                    self.close_review(&mut state, id, "stale")?;
                    return Err(ControlError::ReviewStale(id.clone()));
                }
                Err(error) => return Err(error.into()),
                Ok(false) => return Err(ControlError::ReviewRequired(Box::new(pending.request))),
                Ok(true) => {}
            }
            self.check_review_ceiling(agent, &approver, &call.name, id)?;
            // Consumption is irreversible even if its audit write fails.
            state.close(id);
            self.append_audit_event(
                agent,
                Kind::Observation,
                "review_consumed",
                json!({"review": id, "scope": pending.request.scope}).to_string(),
                vec![pending.request.tape_id],
            )?;
            return Ok(true);
        }
        let attempt = Attempt::new(
            format!("tool:{}", crate::det_seam::Entropy::os().uuid_v4()),
            &subject,
            Capability::exact(
                Polarity::Effect,
                "tool.invoke",
                Resource::new("tool", [call.name.clone()]),
            ),
            facts,
        );
        let cause = if tool.is_some() && tag.permit == Permit::Ask {
            ReviewCause::HighRisk {
                reason: "tool permit requires review".into(),
            }
        } else {
            ReviewCause::PermissionDenied {
                reason: if tool.is_none() {
                    "tool is not granted"
                } else {
                    "tool permit denies execution"
                }
                .into(),
            }
        };
        let request = self.retain_review(
            &mut state,
            agent,
            &subject,
            None,
            attempt,
            cause,
            Some(binding),
        )?;
        Err(ControlError::ReviewRequired(Box::new(request)))
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

fn call_binding(call: &ToolCall) -> Result<CallBinding, ControlError> {
    Ok(CallBinding {
        operation: call.name.clone(),
        input: serde_json::to_string(call)
            .map_err(|e| ControlError::SemanticRecord(e.to_string()))?,
    })
}
