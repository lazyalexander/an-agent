use super::*;

fn dependency(usage: DependencyUse, freshness: Freshness) -> Dependency {
    Dependency {
        fact: FactRef::new("document", "generation-a"),
        usage,
        freshness,
    }
}

fn observation(dependency: &Dependency, revision: Option<&str>) -> DependencyObservation {
    DependencyObservation {
        fact: dependency.fact.clone(),
        state: DependencyState::Available {
            current_revision: revision.map(str::to_owned),
        },
    }
}

#[test]
fn historical_evidence_survives_newer_revisions_but_latest_artifacts_do_not() {
    let pinned = dependency(DependencyUse::ContextEvidence, Freshness::Pinned);
    let latest = dependency(DependencyUse::DerivedArtifact, Freshness::Current);
    let observations = [observation(&pinned, Some("generation-b"))];
    assert_eq!(
        assess_dependencies(&[pinned], &observations).validity,
        Validity::Valid
    );
    assert_eq!(
        assess_dependencies(&[latest], &observations).validity,
        Validity::Stale
    );
}

#[test]
fn authority_and_preconditions_cannot_be_pinned_history() {
    for usage in [
        DependencyUse::Authorization,
        DependencyUse::ExecutionPrecondition,
    ] {
        let pinned = dependency(usage, Freshness::Pinned);
        assert_eq!(
            assess_dependencies(
                std::slice::from_ref(&pinned),
                &[observation(&pinned, Some("generation-a"))]
            )
            .validity,
            Validity::Unknown
        );
    }
}

#[test]
fn incomplete_or_ambiguous_evidence_never_validates() {
    let current = dependency(DependencyUse::Authorization, Freshness::Current);
    for observations in [
        vec![],
        vec![observation(&current, None)],
        vec![observation(&current, Some("generation-a")); 2],
        vec![DependencyObservation {
            fact: current.fact.clone(),
            state: DependencyState::Missing,
        }],
    ] {
        assert_eq!(
            assess_dependencies(std::slice::from_ref(&current), &observations).validity,
            Validity::Unknown
        );
    }
    assert_eq!(assess_dependencies(&[], &[]).validity, Validity::Unknown);
}

#[test]
fn withdrawal_also_invalidates_pinned_history_and_dominates_missing_evidence() {
    let pinned = dependency(DependencyUse::ContextEvidence, Freshness::Pinned);
    let mut missing = pinned.clone();
    missing.fact.kind = "other".into();
    let observed = DependencyObservation {
        fact: pinned.fact.clone(),
        state: DependencyState::Withdrawn,
    };
    assert_eq!(
        assess_dependencies(&[pinned, missing], &[observed]).validity,
        Validity::Stale
    );
}

#[test]
fn round_trip_assessment_and_generation_not_content_identity() {
    let current = dependency(DependencyUse::Authorization, Freshness::Current);
    let fresh = assess_dependencies(
        std::slice::from_ref(&current),
        &[observation(&current, Some("generation-a"))],
    );
    assert_eq!(fresh.validity, Validity::Valid);
    let restored = assess_dependencies(
        std::slice::from_ref(&current),
        &[observation(&current, Some("generation-a-restored"))],
    );
    assert_eq!(restored.validity, Validity::Stale);
    let wire = serde_json::to_string(&restored).unwrap();
    assert_eq!(
        serde_json::from_str::<DependencyAssessment>(&wire).unwrap(),
        restored
    );
}
