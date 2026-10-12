//! Child-to-supervisor requests. Detection has no scheduling power. The
//! trusted host binds the recipient, delivers mail, and drives fresh work.
//! A review response is not a capability grant or an automatic retry.

use std::collections::HashMap;

use an_agent_semantic::{Attempt, Capability, FactRef, Polarity, Resource};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use super::{AgentControl, ControlError};
use crate::act::{Charter, FileFacet, Permit, ToolCall, tag_of};
use crate::agent::Agent;
use crate::memstream::Kind;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReviewCause {
    HighRisk { reason: String },
    PermissionDenied { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewRequest {
    pub id: String,
    pub child: Uuid,
    pub supervisor: Option<Uuid>,
    pub subject: String,
    pub event_id: Option<String>,
    pub attempt: Attempt,
    pub cause: ReviewCause,
    pub tape_id: String,
}

/// Consult keeps the child waiting. Retry schedules nothing and changes no
/// permissions. ApproveOnce is accepted only for an exact Ask tool call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReviewResolution {
    Consult { note: String },
    Retry { note: String },
    Reject { note: String },
    ApproveOnce { note: String },
}

struct PendingReview {
    request: ReviewRequest,
    call: Option<ToolCall>,
    approved: bool,
    approver_card: Option<String>,
}

#[derive(Default)]
pub(super) struct Supervision {
    supervisors: HashMap<Uuid, Uuid>,
    pending: HashMap<String, PendingReview>,
}

impl AgentControl {
    /// Trusted host wiring, not a child-selected address or a delegation
    /// grant. Both threads must be live; cycles and changes during review
    /// are refused. No model or message is executed by this method.
    pub fn set_supervisor(&self, child: Uuid, supervisor: Uuid) -> Result<(), ControlError> {
        self.live(child)?;
        self.live(supervisor)?;
        let mut state = self
            .inner
            .supervision
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if state.pending.values().any(|p| p.request.child == child) {
            return Err(ControlError::SupervisorBinding(
                "child has pending review".into(),
            ));
        }
        let mut cursor = supervisor;
        loop {
            if cursor == child {
                return Err(ControlError::SupervisorBinding("supervision cycle".into()));
            }
            let Some(next) = state.supervisors.get(&cursor) else {
                break;
            };
            cursor = *next;
        }
        let agent = self.live(child)?;
        self.append_audit_event(
            &agent,
            Kind::Observation,
            "supervisor",
            json!({"supervisor": supervisor}).to_string(),
            vec![],
        )?;
        state.supervisors.insert(child, supervisor);
        Ok(())
    }

    /// Waiting requests, including approved calls not yet consumed. The
    /// host decides when to deliver this inbox to the supervisor body.
    pub fn pending_reviews(&self, supervisor: Uuid) -> Result<Vec<ReviewRequest>, ControlError> {
        self.live(supervisor)?;
        let state = self
            .inner
            .supervision
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut requests: Vec<_> = state
            .pending
            .values()
            .filter(|p| p.request.supervisor == Some(supervisor))
            .map(|p| p.request.clone())
            .collect();
        requests.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(requests)
    }

    /// The host authenticates the caller represented by `supervisor`.
    /// Merely deserializing this response conveys no authority. Responses
    /// are mailed to the child; none start a beat or contact a third party.
    pub fn resolve_review(
        &self,
        supervisor: Uuid,
        request_id: &str,
        resolution: ReviewResolution,
    ) -> Result<(), ControlError> {
        let approver = self.live(supervisor)?;
        let mut state = self
            .inner
            .supervision
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let pending = state
            .pending
            .get(request_id)
            .ok_or_else(|| ControlError::ReviewMissing(request_id.into()))?;
        let request = &pending.request;
        self.live(request.child)?;
        if request.supervisor != Some(supervisor)
            || state.supervisors.get(&request.child) != Some(&supervisor)
        {
            return Err(ControlError::ReviewAuthority(request_id.into()));
        }
        if pending.approved && matches!(resolution, ReviewResolution::ApproveOnce { .. }) {
            return Err(ControlError::ReviewAuthority(
                "approval already issued".into(),
            ));
        }
        if matches!(resolution, ReviewResolution::ApproveOnce { .. })
            && (!matches!(request.cause, ReviewCause::HighRisk { .. }) || pending.call.is_none())
        {
            return Err(ControlError::ReviewCeiling(request_id.into()));
        }
        if matches!(resolution, ReviewResolution::ApproveOnce { .. }) {
            let call = pending
                .call
                .as_ref()
                .ok_or_else(|| ControlError::ReviewCeiling(request_id.into()))?;
            let child = self.live(request.child)?;
            let child_tool = child
                .tools()
                .iter()
                .find(|tool| tool.name() == call.name)
                .ok_or_else(|| ControlError::ReviewCeiling(request_id.into()))?;
            let approver_tool = approver
                .tools()
                .iter()
                .find(|tool| tool.name() == call.name)
                .ok_or_else(|| ControlError::ReviewCeiling(request_id.into()))?;
            let child_tag = tag_of(Some(child_tool.as_ref()));
            let approver_tag = tag_of(Some(approver_tool.as_ref()));
            // Use the existing charter vocabulary, with a conservative
            // exact file face here. A broad parent grant may cover it;
            // differing path scopes require a new host authorization.
            let file_fits = child_tag.file == approver_tag.file
                || matches!(child_tag.file, FileFacet::None)
                || matches!(approver_tag.file, FileFacet::Unbounded);
            if child_tag.permit != Permit::Ask
                || approver_tag.permit != Permit::Go
                || !file_fits
                || !Charter::new(approver_tag).allows(&child_tag)
            {
                return Err(ControlError::ReviewCeiling(request_id.into()));
            }
        }
        let message = json!({"review": request_id, "resolution": resolution}).to_string();
        self.send(supervisor, request.child, &message)?;
        let child = self.live(request.child)?;
        self.append_audit_event(
            &child,
            Kind::Observation,
            "review_resolution",
            message,
            vec![request.tape_id.clone()],
        )?;
        match resolution {
            ReviewResolution::Consult { .. } => {
                if let Some(pending) = state.pending.get_mut(request_id) {
                    pending.approved = false;
                    pending.approver_card = None;
                }
            }
            ReviewResolution::ApproveOnce { .. } => {
                if let Some(pending) = state.pending.get_mut(request_id) {
                    pending.approved = true;
                    pending.approver_card = Some(approver.card_hash().into());
                }
            }
            ReviewResolution::Retry { .. } | ReviewResolution::Reject { .. } => {
                state.pending.remove(request_id);
            }
        }
        Ok(())
    }

    pub(super) fn waiting_review(&self, child: Uuid, subject: &str) -> Option<ReviewRequest> {
        self.inner
            .supervision
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pending
            .values()
            .find(|p| p.request.child == child && p.request.subject == subject && p.call.is_none())
            .map(|p| p.request.clone())
    }

    pub(super) fn request_review(
        &self,
        agent: &Agent,
        subject: &str,
        event_id: Option<&str>,
        attempt: Attempt,
        cause: ReviewCause,
        call: Option<ToolCall>,
    ) -> Result<ReviewRequest, ControlError> {
        let mut state = self
            .inner
            .supervision
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(pending) = state.pending.values().find(|p| {
            p.request.child == agent.id()
                && p.request.subject == subject
                && p.call.is_some() == call.is_some()
        }) {
            return Ok(pending.request.clone());
        }
        let supervisor = state.supervisors.get(&agent.id()).copied();
        let mut request = ReviewRequest {
            id: crate::det_seam::Entropy::os().uuid_v4().to_string(),
            child: agent.id(),
            supervisor,
            subject: subject.into(),
            event_id: event_id.map(str::to_string),
            attempt,
            cause,
            tape_id: String::new(),
        };
        request.tape_id = self.append_audit_event(
            agent,
            Kind::Utterance,
            "review_request",
            serde_json::to_string(&request)
                .map_err(|e| ControlError::SemanticRecord(e.to_string()))?,
            request.attempt.refs.clone(),
        )?;
        if let Some(supervisor) = supervisor {
            self.send(
                agent.id(),
                supervisor,
                &serde_json::to_string(&request)
                    .map_err(|e| ControlError::SemanticRecord(e.to_string()))?,
            )?;
            state.pending.insert(
                request.id.clone(),
                PendingReview {
                    request: request.clone(),
                    call,
                    approved: false,
                    approver_card: None,
                },
            );
        }
        // Without a bound supervisor the host receives this request, but
        // no approval is retained. It must wire a live recipient first.
        Ok(request)
    }

    pub(super) fn clear_reviews(&self, child: Uuid, reason: &str) -> Result<(), ControlError> {
        let agent = self.live(child)?;
        let mut state = self
            .inner
            .supervision
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let requests: Vec<_> = state
            .pending
            .values()
            .filter(|p| p.request.child == child)
            .map(|p| p.request.clone())
            .collect();
        for request in requests {
            self.append_audit_event(
                &agent,
                Kind::Observation,
                "review_closed",
                json!({"review": request.id, "reason": reason}).to_string(),
                vec![request.tape_id],
            )?;
            state.pending.remove(&request.id);
        }
        Ok(())
    }

    pub(super) fn review_tool(&self, agent: &Agent, call: &ToolCall) -> Result<bool, ControlError> {
        let tool = agent.tools().iter().find(|tool| tool.name() == call.name);
        let tag = tag_of(tool.map(|tool| tool.as_ref()));
        if tool.is_some() && tag.permit == Permit::Go {
            return Ok(false);
        }
        let subject = format!("tool:{}", call.name);
        let mut facts = vec![
            FactRef::new("agent.card", agent.card_hash()),
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
        {
            let mut state = self
                .inner
                .supervision
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some(pending) = state.pending.values().find(|p| {
                p.request.child == agent.id() && p.request.subject == subject && p.call.is_some()
            }) {
                if !pending.approved {
                    return Err(ControlError::ReviewRequired(Box::new(
                        pending.request.clone(),
                    )));
                }
                if pending.call.as_ref() != Some(call)
                    || pending.request.attempt.facts != attempt.facts
                    || tool.is_none()
                    || tag.permit != Permit::Ask
                {
                    return Err(ControlError::ReviewStale(pending.request.id.clone()));
                }
                let approver =
                    self.live(pending.request.supervisor.ok_or_else(|| {
                        ControlError::ReviewAuthority(pending.request.id.clone())
                    })?)?;
                if pending.approver_card.as_deref() != Some(approver.card_hash()) {
                    return Err(ControlError::ReviewStale(pending.request.id.clone()));
                }
                let request = pending.request.clone();
                self.append_audit_event(
                    agent,
                    Kind::Observation,
                    "review_consumed",
                    json!({"review": request.id}).to_string(),
                    vec![request.tape_id],
                )?;
                state.pending.remove(&request.id);
                return Ok(true);
            }
        }
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
        let request =
            self.request_review(agent, &subject, None, attempt, cause, Some(call.clone()))?;
        Err(ControlError::ReviewRequired(Box::new(request)))
    }
}
