use super::*;
use crate::{Polarity, Resource};

fn fixture() -> (Attempt, BoundaryProfile, Vec<ContractClaim>) {
    let capability =
        Capability::exact(Polarity::Coeffect, "read", Resource::new("document", ["1"]));
    let evidence = vec![FactRef::new("declaration", "fixed")];
    let attempt = Attempt::new("attempt", "worker", capability.clone(), evidence.clone());
    let profile = BoundaryProfile {
        id: "test".into(),
        required_roles: vec!["declaration".into(), "host".into()],
        checked: vec![capability.clone()],
        limitations: vec!["native IO not checked".into()],
    };
    let claims = ["declaration", "host"]
        .into_iter()
        .map(|role| ContractClaim {
            role: role.into(),
            claim: Claim {
                subject: "worker".into(),
                capabilities: vec![capability.clone()],
            },
            evidence: evidence.clone(),
        })
        .collect();
    (attempt, profile, claims)
}

#[test]
fn complete_inputs_allow_only_with_explicit_coverage_and_limitations() {
    let (attempt, profile, claims) = fixture();
    let result = assess_contract(&attempt, &profile, &claims);
    assert_eq!(result.disposition, AssessmentDisposition::Allow);
    assert!(result.findings.is_empty());
    assert_eq!(result.limitations, profile.limitations);
    let json = serde_json::to_value(result).unwrap();
    assert!(json.get("grant").is_none());
}

#[test]
fn missing_authority_role_is_unknown_not_an_allow_from_the_remaining_claim() {
    let (attempt, profile, mut claims) = fixture();
    claims.pop();
    let result = assess_contract(&attempt, &profile, &claims);
    assert_eq!(result.disposition, AssessmentDisposition::Unknown);
    assert!(
        result
            .findings
            .iter()
            .any(|f| f.code == FindingCode::MissingRole && f.role.as_deref() == Some("host"))
    );
    assert_eq!(
        assess_contract(&attempt, &profile, &[]).disposition,
        AssessmentDisposition::Unknown
    );
}

#[test]
fn duplicate_roles_cannot_supply_an_independent_authority() {
    let (attempt, profile, mut claims) = fixture();
    claims[1] = claims[0].clone();
    let result = assess_contract(&attempt, &profile, &claims);
    assert_eq!(result.disposition, AssessmentDisposition::Unknown);
    assert!(
        result
            .findings
            .iter()
            .any(|f| f.code == FindingCode::DuplicateRole)
    );
    assert!(
        result
            .findings
            .iter()
            .any(|f| f.code == FindingCode::MissingRole)
    );
}

#[test]
fn absent_or_wrong_revision_evidence_is_unknown() {
    let (attempt, profile, mut claims) = fixture();
    claims[0].evidence.clear();
    assert_eq!(
        assess_contract(&attempt, &profile, &claims).findings[0].code,
        FindingCode::MissingEvidence
    );
    claims[0]
        .evidence
        .push(FactRef::new("declaration", "other"));
    let result = assess_contract(&attempt, &profile, &claims);
    assert_eq!(result.disposition, AssessmentDisposition::Unknown);
    assert_eq!(result.findings[0].code, FindingCode::UnboundEvidence);
}

#[test]
fn a_claim_does_not_establish_coverage_outside_the_boundary_profile() {
    let (attempt, mut profile, claims) = fixture();
    profile.checked[0].resource = Resource::new("document", ["other"]);
    let result = assess_contract(&attempt, &profile, &claims);
    assert_eq!(result.disposition, AssessmentDisposition::Unknown);
    assert_eq!(result.findings[0].code, FindingCode::UncoveredOperation);
}

#[test]
fn known_violation_remains_deny_even_with_missing_sources() {
    let (attempt, profile, mut claims) = fixture();
    claims.pop();
    claims[0].claim.capabilities.clear();
    let result = assess_contract(&attempt, &profile, &claims);
    assert_eq!(result.disposition, AssessmentDisposition::Deny);
    assert!(
        result
            .findings
            .iter()
            .any(|f| f.code == FindingCode::MissingRole)
    );
    assert!(
        result
            .findings
            .iter()
            .any(|f| f.code == FindingCode::CapabilityNotClaimed)
    );
}

#[test]
fn malformed_profiles_and_attempts_never_allow() {
    let (mut attempt, mut profile, claims) = fixture();
    profile.required_roles.clear();
    assert_eq!(
        assess_contract(&attempt, &profile, &claims).disposition,
        AssessmentDisposition::Unknown
    );
    attempt.v += 1;
    assert_eq!(
        assess_contract(&attempt, &profile, &claims).disposition,
        AssessmentDisposition::Deny
    );
}

#[test]
fn fully_sourced_claim_comparison_matches_existing_rules() {
    let (attempt, profile, claims) = fixture();
    for other_subject in [false, true] {
        for permitted in [false, true] {
            let mut claims = claims.clone();
            if other_subject {
                claims[0].claim.subject = "other".into();
            }
            if !permitted {
                claims[0].claim.capabilities.clear();
            }
            let expected = decide(
                &attempt,
                &claims.iter().map(|c| c.claim.clone()).collect::<Vec<_>>(),
            );
            let actual = assess_contract(&attempt, &profile, &claims);
            assert_eq!(
                actual.disposition == AssessmentDisposition::Allow,
                expected.disposition == Disposition::Allow
            );
        }
    }
}
