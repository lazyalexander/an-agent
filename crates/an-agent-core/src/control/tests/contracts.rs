//! The pure contract checker is compared on real admission paths.

use super::supervision::setup;
use super::*;
use an_agent_semantic::{
    Assessment, AssessmentDisposition, Attempt, BoundaryProfile, Capability, Claim, ContractClaim,
    FactRef, Polarity, Resource, assess_contract,
};

fn check_records(control: &AgentControl, child: Uuid) -> Vec<Value> {
    let records: Vec<Value> = control
        .events(child)
        .unwrap()
        .iter()
        .filter(|event| event.tags.iter().any(|tag| tag == "semantic"))
        .map(|event| serde_json::from_str(&event.content).unwrap())
        .collect();
    for record in &records {
        assert_eq!(record["contract"]["mode"], "shadow");
        assert_eq!(record["contract"]["comparison"], "agree");
        let attempt: Attempt = serde_json::from_value(record["attempt"].clone()).unwrap();
        let profile: BoundaryProfile =
            serde_json::from_value(record["contract"]["profile"].clone()).unwrap();
        let claims: Vec<ContractClaim> =
            serde_json::from_value(record["contract"]["claims"].clone()).unwrap();
        let assessment: Assessment =
            serde_json::from_value(record["contract"]["assessment"].clone()).unwrap();
        assert_eq!(assess_contract(&attempt, &profile, &claims), assessment);
        assert!(!assessment.limitations.is_empty());
        assert!(record["contract"]["assessment"].get("grant").is_none());
        assert_eq!(
            record["decision"]["disposition"],
            record["contract"]["assessment"]["disposition"]
        );
    }
    records
}

struct Echo;
impl SpoolBeat for Echo {
    fn receive(&self, _: &WorkspaceRecord) -> Result<String, String> {
        Ok("reply".into())
    }
}

#[test]
fn routing_allow_and_runtime_denial_have_reproducible_contract_diagnostics() {
    let (_tmp, control, child, _) = setup(Permit::Go);
    control
        .register(&listed(&[("scene", &["body"])], &[], &[]))
        .unwrap();
    control
        .mount_spool("body", Arc::new(Echo), &declares(&["scene"], &[]))
        .unwrap();
    let event = control.push_event("scene", "change").unwrap();
    assert!(
        control
            .offer_on(child, "body", &event.id)
            .unwrap()
            .reply()
            .is_some()
    );
    control
        .register(&listed(&[("scene", &[])], &[], &[]))
        .unwrap();
    assert!(matches!(
        control.offer_on(child, "body", &event.id).unwrap(),
        DeliveryOutcome::NeedsReview(_)
    ));
    let records = check_records(&control, child);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["decision"]["disposition"], "allow");
    assert_eq!(records[1]["decision"]["disposition"], "deny");
    assert_eq!(records[1]["contract"]["profile"]["id"], "core.delivery.v1");
    assert_eq!(
        records[1]["contract"]["claims"][1]["role"],
        "workspace_route"
    );
}

struct AskModel(String);
impl AgentBody for AskModel {
    fn evaluate(&self, projection: &Value) -> Result<BeatStep, String> {
        if projection["steps"] == 0 {
            Ok(BeatStep::InvokeModel {
                clips: vec![self.0.clone()],
            })
        } else {
            Ok(BeatStep::Halt)
        }
    }
}

struct CountModel(Arc<Mutex<usize>>);
impl ModelClient for CountModel {
    fn complete(&self, _: &ModelSpec, _: Vec<ModelMessage>) -> Result<Completion, String> {
        *self.0.lock().unwrap() += 1;
        Ok(Completion {
            content: "result".into(),
            usage: None,
        })
    }
}

#[test]
fn model_preflight_denial_and_executed_call_share_the_contract_checker() {
    let (_tmp, control, child, _) = setup(Permit::Go);
    control
        .register(&listed(&[("scene", &["body"])], &[], &[]))
        .unwrap();
    let event = control.push_event("scene", "change").unwrap();
    let clip = control.note(child, "source", "context").unwrap();
    let calls = Arc::new(Mutex::new(0));
    control.set_model_client(Arc::new(CountModel(calls.clone())));
    control
        .mount_agent(
            "body",
            Arc::new(AskModel(clip.clone())),
            &declares(&["scene"], &[]),
        )
        .unwrap();
    assert!(matches!(
        control.offer_on(child, "body", &event.id).unwrap(),
        DeliveryOutcome::NeedsReview(_)
    ));
    assert_eq!(*calls.lock().unwrap(), 0);
    control.unmount_spool("body").unwrap();
    control
        .mount_agent(
            "body",
            Arc::new(AskModel(clip)),
            &declares_model(&["scene"], &[], crate::act::ModelFacet::Complete),
        )
        .unwrap();
    assert!(matches!(
        control.offer_on(child, "body", &event.id).unwrap(),
        DeliveryOutcome::Halt { .. }
    ));
    assert_eq!(*calls.lock().unwrap(), 1);
    let models: Vec<_> = check_records(&control, child)
        .into_iter()
        .filter(|r| r["contract"]["profile"]["id"] == "core.model.v1")
        .collect();
    assert_eq!(models.len(), 2);
    assert_eq!(models[0]["decision"]["disposition"], "deny");
    assert_eq!(models[1]["decision"]["disposition"], "allow");
    assert_eq!(
        models[1]["contract"]["claims"][1]["role"],
        "configured_endpoint"
    );
}

#[test]
fn shadow_diagnostics_cannot_replace_an_existing_denial() {
    let (_tmp, control, child, _) = setup(Permit::Go);
    let capability = Capability::exact(Polarity::Coeffect, "read", Resource::new("test", ["1"]));
    let evidence = vec![FactRef::new("test", "captured")];
    let attempt = Attempt::new("attempt", "body", capability.clone(), evidence.clone());
    let profile = BoundaryProfile {
        id: "test".into(),
        required_roles: vec!["test".into()],
        checked: vec![capability.clone()],
        limitations: vec![],
    };
    let contracts = [ContractClaim {
        role: "test".into(),
        claim: Claim {
            subject: "body".into(),
            capabilities: vec![capability],
        },
        evidence,
    }];
    let old = [Claim {
        subject: "body".into(),
        capabilities: vec![],
    }];
    let agent = control.live(child).unwrap();
    assert!(matches!(
        control
            .authorize_attempt(&agent, &attempt, &old, &profile, &contracts)
            .unwrap(),
        super::super::semantic::Admission::Denied { .. }
    ));
    let record: Value =
        serde_json::from_str(&control.events(child).unwrap().last().unwrap().content).unwrap();
    assert_eq!(record["contract"]["comparison"], "different");
    let assessment: Assessment =
        serde_json::from_value(record["contract"]["assessment"].clone()).unwrap();
    assert_eq!(assessment.disposition, AssessmentDisposition::Allow);
    assert_eq!(record["decision"]["disposition"], "deny");
}
