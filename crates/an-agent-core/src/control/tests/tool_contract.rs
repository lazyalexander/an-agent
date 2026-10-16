//! Real tool gates retain contract diagnostics without changing authority.

use super::supervision::{call, count_ctor, requested, setup};
use super::*;
use an_agent_semantic::{Assessment, Attempt, BoundaryProfile, ContractClaim, assess_contract};

fn records(control: &AgentControl, child: Uuid) -> Vec<Value> {
    control
        .events(child)
        .unwrap()
        .iter()
        .filter(|event| event.tags.iter().any(|tag| tag == "tool_contract"))
        .map(|event| {
            assert!(event.tags.iter().any(|tag| tag == "audit"));
            assert!(event.act.is_none());
            let record: Value = serde_json::from_str(&event.content).unwrap();
            let attempt: Attempt = serde_json::from_value(record["attempt"].clone()).unwrap();
            let profile: BoundaryProfile =
                serde_json::from_value(record["contract"]["profile"].clone()).unwrap();
            let contracts: Vec<ContractClaim> =
                serde_json::from_value(record["contract"]["claims"].clone()).unwrap();
            let assessment: Assessment =
                serde_json::from_value(record["contract"]["assessment"].clone()).unwrap();
            assert_eq!(assess_contract(&attempt, &profile, &contracts), assessment);
            assert_eq!(record["contract"]["comparison"], "agree");
            assert_eq!(
                record["legacy"],
                record["contract"]["assessment"]["disposition"]
            );
            assert!(!assessment.limitations.is_empty());
            record
        })
        .collect()
}

#[tokio::test]
async fn go_deny_and_missing_tools_have_distinct_gate_evidence() {
    for (permit, missing, ready) in [
        (Permit::Go, false, true),
        (Permit::Deny, false, false),
        (Permit::Go, true, false),
    ] {
        let (_tmp, control, child, _) = setup(permit);
        let mut input = call(1);
        if missing {
            input.name = "unregistered".into();
        }
        let outcome = control.run(child, &input).await;
        if ready {
            assert!(outcome.is_ok());
        } else {
            requested(outcome.err().unwrap());
        }
        let report = records(&control, child);
        assert_eq!(report.len(), 1);
        assert_eq!(report[0]["gate"], if ready { "ready" } else { "denied" });
        assert_eq!(
            control
                .events(child)
                .unwrap()
                .iter()
                .any(|e| e.act.is_some()),
            ready
        );
        assert_eq!(report[0]["contract"]["profile"]["id"], "core.tool.v1");
    }
}

#[tokio::test]
async fn ask_is_waiting_until_consumption_and_never_inherits_the_previous_allow() {
    let (_tmp, control, child, main) = setup(Permit::Ask);
    let request = requested(control.run(child, &call(1)).await.err().unwrap());
    control
        .resolve_review(
            main,
            &request.id,
            ReviewResolution::ApproveOnce {
                note: "exact".into(),
            },
        )
        .unwrap();
    control.run(child, &call(1)).await.unwrap();
    let next = requested(control.run(child, &call(1)).await.err().unwrap());
    assert_ne!(request.id, next.id);
    let report = records(&control, child);
    assert_eq!(
        report
            .iter()
            .map(|r| r["gate"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec![
            "needs_review",
            "approval_eligibility",
            "ready",
            "needs_review"
        ]
    );
    let consumed = report[2]["attempt"]["refs"][0].as_str().unwrap();
    assert!(
        control
            .events(child)
            .unwrap()
            .iter()
            .any(|e| e.id == consumed && e.tags.iter().any(|t| t == "review_consumed"))
    );
    assert_eq!(report[1]["attempt"]["refs"][0], request.tape_id);
}

#[tokio::test]
async fn review_diagnostics_do_not_override_supervisor_permit_or_file_ceiling() {
    for narrow_file in [false, true] {
        let tmp = TempDir::new("tool-contract-ceiling");
        let control = AgentControl::open(tmp.path(), &[("count", count_ctor)]).unwrap();
        let mut child_card = card("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", "count", Permit::Ask);
        if narrow_file {
            child_card.tools[0].tag.file = crate::act::FileFacet::Unbounded;
        }
        let parent_card = card(
            "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
            "count",
            if narrow_file { Permit::Go } else { Permit::Ask },
        );
        let child = control.open_thread(&child_card).unwrap();
        let main = control.open_thread(&parent_card).unwrap();
        control.set_supervisor(child, main).unwrap();
        let request = requested(control.run(child, &call(1)).await.err().unwrap());
        assert!(matches!(
            control.resolve_review(
                main,
                &request.id,
                ReviewResolution::ApproveOnce {
                    note: "cannot widen".into()
                }
            ),
            Err(ControlError::ReviewCeiling(_))
        ));
        let report = records(&control, child);
        let ceiling = report.last().unwrap();
        assert_eq!(
            ceiling["contract"]["profile"]["id"],
            "core.review_ceiling.v1"
        );
        assert_eq!(ceiling["legacy"], "deny");
        let role = if narrow_file {
            "charter_fit"
        } else {
            "supervisor_go_grant"
        };
        assert!(
            ceiling["contract"]["claims"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["role"] == role
                    && c["claim"]["capabilities"].as_array().unwrap().is_empty())
        );
        assert!(
            !control
                .events(child)
                .unwrap()
                .iter()
                .any(|e| e.act.is_some())
        );
    }
}

#[tokio::test]
async fn failed_tool_contract_audit_prevents_go_execution() {
    let (_tmp, control, child, _) = setup(Permit::Go);
    control
        .live(child)
        .unwrap()
        .session()
        .tape()
        .fail_next_append();
    assert!(matches!(
        control.run(child, &call(1)).await,
        Err(ControlError::Store(_))
    ));
    assert!(
        !control
            .events(child)
            .unwrap()
            .iter()
            .any(|e| e.act.is_some())
    );
    control.run(child, &call(1)).await.unwrap();
    assert_eq!(records(&control, child).len(), 1);
}

#[tokio::test]
async fn failure_after_consumption_does_not_restore_approval_or_execute() {
    let (_tmp, control, child, main) = setup(Permit::Ask);
    let old = requested(control.run(child, &call(1)).await.err().unwrap());
    control
        .resolve_review(
            main,
            &old.id,
            ReviewResolution::ApproveOnce {
                note: "exact".into(),
            },
        )
        .unwrap();
    // Consumption succeeds, then the tool-contract observation fails.
    control
        .live(child)
        .unwrap()
        .session()
        .tape()
        .fail_append_after(1);
    assert!(matches!(
        control.run(child, &call(1)).await,
        Err(ControlError::Store(_))
    ));
    assert!(control.pending_reviews(main).unwrap().is_empty());
    let tape = control.events(child).unwrap();
    assert!(
        tape.iter()
            .any(|e| e.tags.iter().any(|t| t == "review_consumed"))
    );
    assert!(!tape.iter().any(|e| e.act.is_some()));
    let fresh = requested(control.run(child, &call(1)).await.err().unwrap());
    assert_ne!(old.id, fresh.id);
    assert!(
        records(&control, child)
            .iter()
            .all(|r| r["gate"] != "ready")
    );
}
