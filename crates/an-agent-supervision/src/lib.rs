//! Transport-neutral supervision state. The caller authenticates principals,
//! serializes transitions, persists evidence, and checks resource ceilings.
//! This module performs no IO and never schedules or executes an operation.

use std::collections::{HashMap, HashSet};

use an_agent_semantic::{Attempt, FactRef};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// One live engagement, distinct even when a sealed session is resumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InstanceRef {
    pub principal: Uuid,
    pub incarnation: Uuid,
}

#[cfg(test)]
mod tests;

/// Immutable authority and mount inputs of one retained request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewScope {
    pub child: InstanceRef,
    pub supervisor: Option<InstanceRef>,
    pub mount: Option<Uuid>,
    pub child_epoch: u64,
    pub supervisor_epoch: Option<u64>,
}

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
    /// Legacy evidence remains readable but cannot enter the active ledger.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<ReviewScope>,
}

/// Responses carry intent only. None schedule work or expand a hard ceiling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReviewResolution {
    Consult { note: String },
    Retry { note: String },
    Reject { note: String },
    ApproveOnce { note: String },
}

/// Opaque boundary input. The state machine does not interpret its payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallBinding {
    pub operation: String,
    pub input: String,
}

#[derive(Debug, Clone)]
pub struct PendingReview {
    pub request: ReviewRequest,
    pub call: Option<CallBinding>,
    approver_card: Option<String>,
}

impl PendingReview {
    pub fn approved(&self) -> bool {
        self.approver_card.is_some()
    }
}

#[derive(Debug, Error)]
pub enum ReviewError {
    #[error("invalid supervision binding: {0}")]
    Binding(String),
    #[error("principal is not active: {0}")]
    Inactive(Uuid),
    #[error("review request is not pending: {0}")]
    Missing(String),
    #[error("review authority does not match: {0}")]
    Authority(String),
    #[error("review scope or inputs are stale: {0}")]
    Stale(String),
    #[error("review cannot authorize this operation: {0}")]
    Ceiling(String),
}

/// Trusted process-local ledger, deliberately not serializable. Historical
/// records cannot restore authority by deserializing a request or approval.
#[derive(Default)]
pub struct Supervision {
    instances: HashMap<Uuid, (InstanceRef, u64)>,
    mounts: HashSet<Uuid>,
    supervisors: HashMap<InstanceRef, InstanceRef>,
    pending: HashMap<String, PendingReview>,
    seen: HashSet<String>,
}

impl Supervision {
    pub fn open(&mut self, instance: InstanceRef) -> Result<(), ReviewError> {
        if self.instances.contains_key(&instance.principal) {
            return Err(ReviewError::Binding("principal already active".into()));
        }
        self.instances.insert(instance.principal, (instance, 0));
        Ok(())
    }

    pub fn instance(&self, principal: Uuid) -> Result<InstanceRef, ReviewError> {
        self.instances
            .get(&principal)
            .map(|(instance, _)| *instance)
            .ok_or(ReviewError::Inactive(principal))
    }

    pub fn mount(&mut self, id: Uuid) {
        self.mounts.insert(id);
    }

    pub fn mounted(&self, id: Uuid) -> bool {
        self.mounts.contains(&id)
    }

    pub fn unmount(&mut self, id: Uuid) -> Vec<ReviewRequest> {
        self.mounts.remove(&id);
        self.remove_where(|p| {
            p.request
                .scope
                .as_ref()
                .is_some_and(|s| s.mount == Some(id))
        })
    }

    pub fn check_binding(&self, child: Uuid, supervisor: Uuid) -> Result<(), ReviewError> {
        let child = self.instance(child)?;
        let mut cursor = self.instance(supervisor)?;
        if self
            .pending
            .values()
            .any(|p| p.request.child == child.principal)
        {
            return Err(ReviewError::Binding("child has pending review".into()));
        }
        loop {
            if cursor == child {
                return Err(ReviewError::Binding("supervision cycle".into()));
            }
            let Some(next) = self.supervisors.get(&cursor) else {
                return Ok(());
            };
            cursor = *next;
        }
    }

    pub fn bind(&mut self, child: Uuid, supervisor: Uuid) -> Result<(), ReviewError> {
        self.check_binding(child, supervisor)?;
        self.supervisors
            .insert(self.instance(child)?, self.instance(supervisor)?);
        Ok(())
    }

    pub fn scope(&self, child: Uuid, mount: Option<Uuid>) -> Result<ReviewScope, ReviewError> {
        let (child, child_epoch) = self
            .instances
            .get(&child)
            .ok_or(ReviewError::Inactive(child))?;
        if let Some(id) = mount
            && !self.mounted(id)
        {
            return Err(ReviewError::Stale(id.to_string()));
        }
        let supervisor = self.supervisors.get(child).copied();
        let supervisor_epoch = supervisor.map(|parent| self.instances[&parent.principal].1);
        Ok(ReviewScope {
            child: *child,
            supervisor,
            mount,
            child_epoch: *child_epoch,
            supervisor_epoch,
        })
    }

    pub fn current(&self, scope: &ReviewScope) -> bool {
        self.scope(scope.child.principal, scope.mount)
            .is_ok_and(|current| current == *scope)
    }

    pub fn waiting(
        &self,
        scope: &ReviewScope,
        subject: &str,
        call: bool,
    ) -> Option<&PendingReview> {
        self.pending.values().find(|p| {
            p.request.scope.as_ref() == Some(scope)
                && p.request.subject == subject
                && p.call.is_some() == call
        })
    }

    pub fn pending(&self, supervisor: Uuid) -> Vec<ReviewRequest> {
        let mut requests: Vec<_> = self
            .pending
            .values()
            .filter(|p| p.request.supervisor == Some(supervisor))
            .map(|p| p.request.clone())
            .collect();
        requests.sort_by(|a, b| a.id.cmp(&b.id));
        requests
    }

    pub fn get(&self, id: &str) -> Result<&PendingReview, ReviewError> {
        self.pending
            .get(id)
            .ok_or_else(|| ReviewError::Missing(id.into()))
    }

    pub fn insert(
        &mut self,
        request: ReviewRequest,
        call: Option<CallBinding>,
    ) -> Result<(), ReviewError> {
        let scope = request
            .scope
            .as_ref()
            .ok_or_else(|| ReviewError::Stale(request.id.clone()))?;
        if !self.current(scope)
            || scope.child.principal != request.child
            || scope.supervisor.map(|s| s.principal) != request.supervisor
            || scope.supervisor.is_none()
        {
            return Err(ReviewError::Stale(request.id));
        }
        if self.seen.contains(&request.id)
            || self
                .waiting(scope, &request.subject, call.is_some())
                .is_some()
        {
            return Err(ReviewError::Binding("review already pending".into()));
        }
        self.seen.insert(request.id.clone());
        self.pending.insert(
            request.id.clone(),
            PendingReview {
                request,
                call,
                approver_card: None,
            },
        );
        Ok(())
    }

    pub fn check_resolution(
        &self,
        supervisor: Uuid,
        id: &str,
        resolution: &ReviewResolution,
    ) -> Result<(), ReviewError> {
        let pending = self.get(id)?;
        let scope = pending
            .request
            .scope
            .as_ref()
            .ok_or_else(|| ReviewError::Stale(id.into()))?;
        if !self.current(scope) {
            return Err(ReviewError::Stale(id.into()));
        }
        if scope.supervisor != Some(self.instance(supervisor)?) {
            return Err(ReviewError::Authority(id.into()));
        }
        if matches!(resolution, ReviewResolution::ApproveOnce { .. }) {
            if pending.approved() {
                return Err(ReviewError::Authority("approval already issued".into()));
            }
            if pending.call.is_none()
                || !matches!(pending.request.cause, ReviewCause::HighRisk { .. })
            {
                return Err(ReviewError::Ceiling(id.into()));
            }
        }
        Ok(())
    }

    /// The caller persists approval evidence before this transition, and
    /// applies revocations before attempting fallible evidence delivery.
    pub fn resolve(
        &mut self,
        supervisor: Uuid,
        id: &str,
        resolution: &ReviewResolution,
        card: &str,
    ) -> Result<(), ReviewError> {
        self.check_resolution(supervisor, id, resolution)?;
        match resolution {
            ReviewResolution::ApproveOnce { .. } | ReviewResolution::Consult { .. } => {
                let pending = self
                    .pending
                    .get_mut(id)
                    .ok_or_else(|| ReviewError::Missing(id.into()))?;
                pending.approver_card = matches!(resolution, ReviewResolution::ApproveOnce { .. })
                    .then(|| card.to_string());
            }
            ReviewResolution::Retry { .. } | ReviewResolution::Reject { .. } => {
                self.close(id);
            }
        }
        Ok(())
    }

    /// Validate before consuming. A mismatch invalidates even an unapproved
    /// request; it must not keep a changed operation waiting on stale input.
    pub fn check_call(
        &self,
        id: &str,
        call: &CallBinding,
        facts: &[FactRef],
        approver_card: &str,
    ) -> Result<bool, ReviewError> {
        let pending = self.get(id)?;
        let scope = pending
            .request
            .scope
            .as_ref()
            .ok_or_else(|| ReviewError::Stale(id.into()))?;
        if !self.current(scope)
            || pending.call.as_ref() != Some(call)
            || pending.request.attempt.facts != facts
        {
            return Err(ReviewError::Stale(id.into()));
        }
        if let Some(card) = &pending.approver_card
            && card != approver_card
        {
            return Err(ReviewError::Stale(id.into()));
        }
        Ok(pending.approved())
    }

    pub fn close(&mut self, id: &str) -> Option<ReviewRequest> {
        self.pending.remove(id).map(|p| p.request)
    }

    /// Advance the authority epoch and remove all reviews involving it.
    /// On epoch exhaustion the instance retires rather than wrapping.
    pub fn revoke(&mut self, principal: Uuid) -> Result<Vec<ReviewRequest>, ReviewError> {
        let (instance, epoch) = *self
            .instances
            .get(&principal)
            .ok_or(ReviewError::Inactive(principal))?;
        let Some(next) = epoch.checked_add(1) else {
            return Ok(self.retire(principal));
        };
        self.instances.insert(principal, (instance, next));
        Ok(self.remove_where(|p| {
            p.request.child == principal || p.request.supervisor == Some(principal)
        }))
    }

    pub fn retire(&mut self, principal: Uuid) -> Vec<ReviewRequest> {
        if let Some((instance, _)) = self.instances.remove(&principal) {
            self.supervisors
                .retain(|child, parent| *child != instance && *parent != instance);
        }
        self.remove_where(|p| {
            p.request.child == principal || p.request.supervisor == Some(principal)
        })
    }

    pub fn invalidate_fact(&mut self, kind: &str, revision: &str) -> Vec<ReviewRequest> {
        self.remove_where(|p| {
            p.request
                .attempt
                .facts
                .iter()
                .any(|f| f.kind == kind && f.revision != revision)
        })
    }

    fn remove_where(&mut self, predicate: impl Fn(&PendingReview) -> bool) -> Vec<ReviewRequest> {
        let mut ids: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, pending)| predicate(pending))
            .map(|(id, _)| id.clone())
            .collect();
        ids.sort();
        ids.into_iter().filter_map(|id| self.close(&id)).collect()
    }
}
