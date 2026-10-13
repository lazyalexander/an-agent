//! Transport-neutral semantics for effects and contextual dependencies.
//!
//! Business layers translate their operations into an [`Attempt`]. Policy
//! compares that concrete attempt with independent [`Claim`]s and returns a
//! version-bound [`Decision`]. This crate owns no tools, tapes, workspaces, or
//! interaction loops.

use serde::{Deserialize, Serialize};

mod contract;
pub use contract::{
    Assessment, AssessmentDisposition, BoundaryProfile, ContractClaim, Finding, FindingCode,
    assess_contract,
};

pub const SCHEMA_VERSION: u16 = 1;

/// Whether an operation consumes context or changes/discloses the world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Polarity {
    Coeffect,
    Effect,
}

/// A transport-neutral resource name. Adapters, not this crate, canonicalize
/// filesystem paths, URLs, memory ids, and host object ids into segments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resource {
    pub namespace: String,
    pub segments: Vec<String>,
}

impl Resource {
    pub fn new(
        namespace: impl Into<String>,
        segments: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            namespace: namespace.into(),
            segments: segments.into_iter().map(Into::into).collect(),
        }
    }
}

/// One operation over one resource. `recursive` means the claim also covers
/// descendants by whole resource segments; it is never a string prefix test.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    pub polarity: Polarity,
    pub operation: String,
    pub resource: Resource,
    #[serde(default)]
    pub recursive: bool,
}

impl Capability {
    pub fn exact(polarity: Polarity, operation: impl Into<String>, resource: Resource) -> Self {
        Self {
            polarity,
            operation: operation.into(),
            resource,
            recursive: false,
        }
    }

    pub fn covers(&self, attempted: &Self) -> bool {
        if self.polarity != attempted.polarity
            || self.operation != attempted.operation
            || self.resource.namespace != attempted.resource.namespace
            || (attempted.recursive && !self.recursive)
        {
            return false;
        }
        self.resource.segments == attempted.resource.segments
            || (self.recursive
                && attempted
                    .resource
                    .segments
                    .starts_with(&self.resource.segments))
    }
}

/// A statement made by one independent authority. Authorization intersects
/// claims: every claim supplied to [`decide`] must cover the attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub subject: String,
    pub capabilities: Vec<Capability>,
}

impl Claim {
    pub fn allows(&self, attempt: &Attempt) -> bool {
        self.subject == attempt.subject
            && self
                .capabilities
                .iter()
                .any(|capability| capability.covers(&attempt.capability))
    }
}

/// One immutable input to a policy decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactRef {
    pub kind: String,
    pub revision: String,
}

impl FactRef {
    pub fn new(kind: impl Into<String>, revision: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            revision: revision.into(),
        }
    }
}

/// A concrete operation at a trusted boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attempt {
    pub v: u16,
    pub id: String,
    pub subject: String,
    pub capability: Capability,
    pub facts: Vec<FactRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<String>,
}

impl Attempt {
    pub fn new(
        id: impl Into<String>,
        subject: impl Into<String>,
        capability: Capability,
        facts: Vec<FactRef>,
    ) -> Self {
        Self {
            v: SCHEMA_VERSION,
            id: id.into(),
            subject: subject.into(),
            capability,
            facts,
            refs: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Disposition {
    Allow,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DenyReason {
    InvalidAttempt,
    MissingClaim,
    SubjectMismatch,
    CapabilityNotClaimed,
}

impl DenyReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidAttempt => "invalid_attempt",
            Self::MissingClaim => "missing_claim",
            Self::SubjectMismatch => "subject_mismatch",
            Self::CapabilityNotClaimed => "capability_not_claimed",
        }
    }
}

/// A grant is useful only with the exact attempt and fact revisions copied
/// here. An adapter must not widen it or reuse it for a later epoch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    pub attempt_id: String,
    /// Older grants can still be read, but an absent subject cannot authorize.
    #[serde(default)]
    pub subject: String,
    pub capability: Capability,
    pub facts: Vec<FactRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<String>,
}

impl Grant {
    /// Exact binding, not capability coverage. References bind the inputs too.
    /// Authenticity, expiry and single-use consumption belong to the trusted
    /// boundary; a deserialized grant is not by itself an authority token.
    pub fn matches(&self, attempt: &Attempt) -> bool {
        attempt.v == SCHEMA_VERSION
            && self.attempt_id == attempt.id
            && self.subject == attempt.subject
            && self.capability == attempt.capability
            && self.facts == attempt.facts
            && self.refs == attempt.refs
    }
}

/// What the boundary can establish about execution. `Unknown` includes a
/// transport failure after a request may have reached the other side; it is
/// never a license to retry an irreversible operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Succeeded,
    Failed,
    Unknown,
}

/// A boundary's execution report, anchored to the exact grant and evidence.
/// This records a claim, not a proof that the external world was rolled back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub v: u16,
    pub grant: Grant,
    pub outcome: Outcome,
    pub evidence: Vec<FactRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<String>,
}

impl Receipt {
    pub fn new(grant: Grant, outcome: Outcome, evidence: Vec<FactRef>, refs: Vec<String>) -> Self {
        Self {
            v: SCHEMA_VERSION,
            grant,
            outcome,
            evidence,
            refs,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    pub v: u16,
    pub attempt_id: String,
    pub disposition: Disposition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<DenyReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant: Option<Grant>,
}

/// Default-deny intersection of independent claims. It is intentionally a
/// pure function: interaction and evidence recording belong to adapters.
pub fn decide(attempt: &Attempt, claims: &[Claim]) -> Decision {
    let invalid = attempt.v != SCHEMA_VERSION
        || attempt.id.is_empty()
        || attempt.subject.is_empty()
        || attempt.capability.operation.is_empty()
        || attempt.capability.resource.namespace.is_empty()
        || attempt
            .facts
            .iter()
            .any(|fact| fact.kind.is_empty() || fact.revision.is_empty());
    let reason = if invalid {
        Some(DenyReason::InvalidAttempt)
    } else if claims.is_empty() {
        Some(DenyReason::MissingClaim)
    } else if claims.iter().any(|claim| claim.subject != attempt.subject) {
        Some(DenyReason::SubjectMismatch)
    } else if claims.iter().any(|claim| !claim.allows(attempt)) {
        Some(DenyReason::CapabilityNotClaimed)
    } else {
        None
    };

    match reason {
        Some(reason) => Decision {
            v: SCHEMA_VERSION,
            attempt_id: attempt.id.clone(),
            disposition: Disposition::Deny,
            reason: Some(reason),
            grant: None,
        },
        None => Decision {
            v: SCHEMA_VERSION,
            attempt_id: attempt.id.clone(),
            disposition: Disposition::Allow,
            reason: None,
            grant: Some(Grant {
                attempt_id: attempt.id.clone(),
                subject: attempt.subject.clone(),
                capability: attempt.capability.clone(),
                facts: attempt.facts.clone(),
                refs: attempt.refs.clone(),
            }),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(path: &[&str], recursive: bool) -> Capability {
        Capability {
            polarity: Polarity::Coeffect,
            operation: "fs.read".into(),
            resource: Resource::new("workspace", path.iter().copied()),
            recursive,
        }
    }

    fn attempt(capability: Capability) -> Attempt {
        Attempt::new(
            "attempt-1",
            "worker",
            capability,
            vec![FactRef::new("policy", "sha256:abc")],
        )
    }

    #[test]
    fn recursive_scope_uses_segments_not_string_prefixes() {
        let root = read(&["src"], true);
        assert!(root.covers(&read(&["src", "main.rs"], false)));
        assert!(!root.covers(&read(&["src-other", "main.rs"], false)));
        assert!(!read(&["src"], false).covers(&read(&["src", "main.rs"], false)));
    }

    #[test]
    fn every_independent_claim_must_cover_the_attempt() {
        let attempt = attempt(read(&["src", "main.rs"], false));
        let broad = Claim {
            subject: "worker".into(),
            capabilities: vec![read(&["src"], true)],
        };
        let exact = Claim {
            subject: "worker".into(),
            capabilities: vec![attempt.capability.clone()],
        };
        let wrong = Claim {
            subject: "worker".into(),
            capabilities: vec![read(&["tests"], true)],
        };

        let allowed = decide(&attempt, &[broad.clone(), exact]);
        assert_eq!(allowed.disposition, Disposition::Allow);
        assert_eq!(allowed.grant.unwrap().facts, attempt.facts);

        let denied = decide(&attempt, &[broad, wrong]);
        assert_eq!(denied.disposition, Disposition::Deny);
        assert_eq!(denied.reason, Some(DenyReason::CapabilityNotClaimed));
        assert!(denied.grant.is_none());
    }

    #[test]
    fn missing_and_wrong_subject_claims_fail_closed() {
        let attempt = attempt(read(&["src", "main.rs"], false));
        assert_eq!(decide(&attempt, &[]).reason, Some(DenyReason::MissingClaim));
        let claim = Claim {
            subject: "someone-else".into(),
            capabilities: vec![attempt.capability.clone()],
        };
        assert_eq!(
            decide(&attempt, &[claim]).reason,
            Some(DenyReason::SubjectMismatch)
        );
    }

    #[test]
    fn exact_claim_cannot_authorize_recursive_attempt() {
        let exact = read(&["src"], false);
        assert!(!exact.covers(&read(&["src"], true)));
        assert!(read(&["src"], true).covers(&read(&["src", "nested"], true)));
    }

    #[test]
    fn grant_cannot_be_reused_after_subject_inputs_or_revisions_change() {
        let mut original = attempt(read(&["src"], false));
        original.refs.push("source-event".into());
        let claim = Claim {
            subject: original.subject.clone(),
            capabilities: vec![original.capability.clone()],
        };
        let grant = decide(&original, &[claim]).grant.unwrap();
        assert!(grant.matches(&original));
        let mut changed = original.clone();
        changed.subject = "other-worker".into();
        assert!(!grant.matches(&changed));
        changed = original.clone();
        changed.facts[0].revision = "sha256:new".into();
        assert!(!grant.matches(&changed));
        changed = original.clone();
        changed.refs[0] = "other-source".into();
        assert!(!grant.matches(&changed));
        changed = original.clone();
        changed.id = "retry".into();
        assert!(!grant.matches(&changed));
        changed = original;
        changed.capability.recursive = true;
        assert!(!grant.matches(&changed));
    }

    #[test]
    fn unknown_schema_and_incomplete_attempts_fail_closed() {
        let original = attempt(read(&["src"], false));
        let claim = Claim {
            subject: original.subject.clone(),
            capabilities: vec![original.capability.clone()],
        };
        let mut changed = original.clone();
        changed.v = SCHEMA_VERSION + 1;
        assert_eq!(
            decide(&changed, std::slice::from_ref(&claim)).reason,
            Some(DenyReason::InvalidAttempt)
        );
        changed = original;
        changed.facts[0].revision.clear();
        assert_eq!(
            decide(&changed, &[claim]).reason,
            Some(DenyReason::InvalidAttempt)
        );
    }

    #[test]
    fn legacy_unbound_grant_reads_but_cannot_authorize() {
        let original = attempt(read(&["src"], false));
        let grant: Grant = serde_json::from_value(serde_json::json!({
            "attempt_id": original.id,
            "capability": original.capability,
            "facts": original.facts,
        }))
        .unwrap();
        assert!(!grant.matches(&original));
    }
}
