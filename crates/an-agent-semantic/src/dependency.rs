//! Pure dependency validity. Observations are supplied by a trusted boundary,
//! not inferred from event order or from self-asserted provenance labels.

use serde::{Deserialize, Serialize};

use crate::FactRef;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyUse {
    Authorization,
    ExecutionPrecondition,
    ContextEvidence,
    DerivedArtifact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Freshness {
    Current,
    Pinned,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dependency {
    pub fact: FactRef,
    pub usage: DependencyUse,
    pub freshness: Freshness,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DependencyState {
    /// The exact referenced revision is retained and usable. A latest pointer
    /// is needed only for Current dependencies; None means it is unknown.
    Available {
        current_revision: Option<String>,
    },
    Withdrawn,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyObservation {
    pub fact: FactRef,
    pub state: DependencyState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Validity {
    Valid,
    Stale,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyIssue {
    EmptySet,
    InvalidDependency,
    MissingObservation,
    AmbiguousObservation,
    Unavailable,
    UnknownCurrentRevision,
    RevisionChanged,
    Withdrawn,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyFinding {
    pub dependency: Option<usize>,
    pub issue: DependencyIssue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyAssessment {
    pub checker: String,
    pub validity: Validity,
    pub findings: Vec<DependencyFinding>,
}

/// No scheduling, revocation, confidence score, or IO. Valid means only that
/// these dependencies satisfy this set of boundary-supplied observations.
/// It does not establish complete coverage, authenticity, or an execution grant.
pub fn assess_dependencies(
    dependencies: &[Dependency],
    observations: &[DependencyObservation],
) -> DependencyAssessment {
    let mut findings = Vec::new();
    if dependencies.is_empty() {
        findings.push(DependencyFinding {
            dependency: None,
            issue: DependencyIssue::EmptySet,
        });
    }
    for (index, dependency) in dependencies.iter().enumerate() {
        let mut report = |issue| {
            findings.push(DependencyFinding {
                dependency: Some(index),
                issue,
            })
        };
        if dependency.fact.kind.trim().is_empty()
            || dependency.fact.revision.trim().is_empty()
            || (dependency.freshness == Freshness::Pinned
                && matches!(
                    dependency.usage,
                    DependencyUse::Authorization | DependencyUse::ExecutionPrecondition
                ))
        {
            report(DependencyIssue::InvalidDependency);
            continue;
        }
        let mut matching = observations.iter().filter(|o| o.fact == dependency.fact);
        let Some(observation) = matching.next() else {
            report(DependencyIssue::MissingObservation);
            continue;
        };
        if matching.next().is_some() {
            report(DependencyIssue::AmbiguousObservation);
            continue;
        }
        match &observation.state {
            DependencyState::Withdrawn => report(DependencyIssue::Withdrawn),
            DependencyState::Missing => report(DependencyIssue::Unavailable),
            DependencyState::Available { current_revision } => {
                if dependency.freshness == Freshness::Current {
                    match current_revision.as_deref() {
                        None | Some("") => report(DependencyIssue::UnknownCurrentRevision),
                        Some(revision) if revision != dependency.fact.revision => {
                            report(DependencyIssue::RevisionChanged)
                        }
                        Some(_) => {}
                    }
                }
            }
        }
    }
    let validity = if findings.iter().any(|f| {
        matches!(
            f.issue,
            DependencyIssue::RevisionChanged | DependencyIssue::Withdrawn
        )
    }) {
        Validity::Stale
    } else if findings.is_empty() {
        Validity::Valid
    } else {
        Validity::Unknown
    };
    DependencyAssessment {
        checker: "dependency-v1".into(),
        validity,
        findings,
    }
}

#[cfg(test)]
mod tests;
