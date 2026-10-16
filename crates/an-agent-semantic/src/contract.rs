//! Pure declaration checks. Coverage is supplied by a trusted adapter, not
//! inferred from a descriptor, an absence of observations, or a package name.

use serde::{Deserialize, Serialize};

use crate::{Attempt, Capability, Claim, DenyReason, Disposition, FactRef, decide};

/// Which claim roles a boundary requires and which operations it checks.
/// `checked` describes admission coverage only, not all effects of a body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundaryProfile {
    pub id: String,
    pub required_roles: Vec<String>,
    pub checked: Vec<Capability>,
    pub limitations: Vec<String>,
}

/// A normalized claim with its role and exact input evidence. The role is
/// assigned by the trusted adapter, never by an untrusted descriptor itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractClaim {
    pub role: String,
    pub claim: Claim,
    pub evidence: Vec<FactRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssessmentDisposition {
    Allow,
    Deny,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingCode {
    InvalidProfile,
    MissingRole,
    DuplicateRole,
    MissingEvidence,
    UnboundEvidence,
    UncoveredOperation,
    InvalidAttempt,
    SubjectMismatch,
    CapabilityNotClaimed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub code: FindingCode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

/// A diagnostic, deliberately without a grant or executable handle. Allow
/// means the supplied declarations cover this attempt within this profile.
/// It does not authenticate sources, establish freshness, or prove native IO.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assessment {
    pub checker: String,
    pub attempt_id: String,
    pub profile: String,
    pub disposition: AssessmentDisposition,
    pub findings: Vec<Finding>,
    pub limitations: Vec<String>,
}

/// Shared by offline preflight and runtime comparison. A missing source or
/// coverage gap is unknown; a demonstrated violation is deny even if other
/// information is missing. Every supplied claim must cover the operation.
pub fn assess_contract(
    attempt: &Attempt,
    profile: &BoundaryProfile,
    claims: &[ContractClaim],
) -> Assessment {
    let mut findings = Vec::new();
    let mut add = |code, role: Option<&str>| {
        findings.push(Finding {
            code,
            role: role.map(str::to_string),
        });
    };
    if profile.id.trim().is_empty()
        || profile.required_roles.is_empty()
        || profile
            .required_roles
            .iter()
            .enumerate()
            .any(|(i, role)| role.trim().is_empty() || profile.required_roles[..i].contains(role))
    {
        add(FindingCode::InvalidProfile, None);
    }
    for role in &profile.required_roles {
        if !claims.iter().any(|claim| &claim.role == role) {
            add(FindingCode::MissingRole, Some(role));
        }
    }
    for (i, source) in claims.iter().enumerate() {
        if source.role.trim().is_empty() {
            add(FindingCode::InvalidProfile, Some(&source.role));
        }
        if claims[..i].iter().any(|other| other.role == source.role) {
            add(FindingCode::DuplicateRole, Some(&source.role));
        }
        if source.evidence.is_empty() {
            add(FindingCode::MissingEvidence, Some(&source.role));
        } else if source
            .evidence
            .iter()
            .any(|fact| !attempt.facts.contains(fact))
        {
            add(FindingCode::UnboundEvidence, Some(&source.role));
        }
    }
    if !profile
        .checked
        .iter()
        .any(|capability| capability.covers(&attempt.capability))
    {
        add(FindingCode::UncoveredOperation, None);
    }
    let plain: Vec<_> = claims.iter().map(|source| source.claim.clone()).collect();
    let decision = decide(attempt, &plain);
    let violation = match decision.reason {
        Some(DenyReason::InvalidAttempt) => Some(FindingCode::InvalidAttempt),
        Some(DenyReason::SubjectMismatch) => Some(FindingCode::SubjectMismatch),
        Some(DenyReason::CapabilityNotClaimed) => Some(FindingCode::CapabilityNotClaimed),
        Some(DenyReason::MissingClaim) | None => None,
    };
    if let Some(code) = violation {
        add(code, None);
    }
    let disposition = if violation.is_some() {
        AssessmentDisposition::Deny
    } else if findings.is_empty() && decision.disposition == Disposition::Allow {
        AssessmentDisposition::Allow
    } else {
        AssessmentDisposition::Unknown
    };
    Assessment {
        checker: "contract-v1".into(),
        attempt_id: attempt.id.clone(),
        profile: profile.id.clone(),
        disposition,
        findings,
        limitations: profile.limitations.clone(),
    }
}

#[cfg(test)]
mod tests;
