//! Shadow projections of existing tool and review ceilings. These records
//! describe trusted boundary decisions, not OS isolation or new authority.

use an_agent_semantic::{
    AssessmentDisposition, Attempt, BoundaryProfile, Capability, Claim, ContractClaim, FactRef,
    Polarity, Resource, assess_contract,
};
use serde_json::json;

use super::{AgentControl, ControlError, ReviewRequest, ReviewScope};
use crate::act::{Charter, FileFacet, Permit, ToolCall, tag_of};
use crate::agent::Agent;
use crate::memstream::Kind;

enum ToolBoundary {
    Invoke,
    ReviewCeiling,
}

impl ToolBoundary {
    fn profile(self) -> BoundaryProfile {
        let (id, roles, operation, namespace, limitations) = match self {
            Self::Invoke => (
                "core.tool.v1",
                vec!["bound_tool_grant", "review_gate"],
                "tool.invoke",
                "tool",
                vec![
                    "Checks tool presence, bound permit and the existing exact-review gate, not actual file/network/process effects.",
                    "Ready describes local admission, not successful execution or an external idempotency guarantee.",
                ],
            ),
            Self::ReviewCeiling => (
                "core.review_ceiling.v1",
                vec!["child_ask_grant", "supervisor_go_grant", "charter_fit"],
                "review.approve_once",
                "review",
                vec![
                    "Projects the existing charter predicate and conservative file-face match, not filesystem canonicalization or enforcement.",
                    "Eligibility is not approval activation; evidence delivery and the live review ledger still govern activation.",
                ],
            ),
        };
        BoundaryProfile {
            id: id.into(),
            required_roles: roles.into_iter().map(str::to_string).collect(),
            checked: vec![Capability {
                polarity: Polarity::Effect,
                operation: operation.into(),
                resource: Resource::new(namespace, std::iter::empty::<&str>()),
                recursive: true,
            }],
            limitations: limitations.into_iter().map(str::to_string).collect(),
        }
    }
}

impl AgentControl {
    /// Called after the legacy gate has classified the request. A consumed
    /// approval stays consumed if this evidence write fails.
    pub(super) fn record_tool_gate(
        &self,
        agent: &Agent,
        call: &ToolCall,
        scope: &ReviewScope,
        review: Option<&str>,
        consumed: Option<&str>,
        legacy_allowed: bool,
    ) -> Result<(), ControlError> {
        let tool = agent.tools().iter().find(|tool| tool.name() == call.name);
        let tag = tag_of(tool.map(|tool| tool.as_ref()));
        let grant_fits = tool.is_some() && tag.permit != Permit::Deny;
        let review_fits = tag.permit != Permit::Ask || consumed.is_some();
        let gate = if legacy_allowed {
            "ready"
        } else if tool.is_some() && tag.permit == Permit::Ask {
            "needs_review"
        } else {
            "denied"
        };
        let capability = Capability::exact(
            Polarity::Effect,
            "tool.invoke",
            Resource::new("tool", [call.name.as_str()]),
        );
        let grant = self.semantic_snapshot(
            agent,
            "tool.grant",
            &json!({"present": tool.is_some(), "tag": tag}),
        )?;
        let review_fact = self.semantic_snapshot(
            agent,
            "tool.review_gate",
            &json!({"scope": scope, "review": review, "consumed": consumed, "gate": gate}),
        )?;
        let card = FactRef::new("agent.card", agent.card_hash());
        let mut attempt = Attempt::new(
            format!("tool-check:{}", crate::det_seam::Entropy::os().uuid_v4()),
            format!("tool:{}", call.name),
            capability.clone(),
            vec![
                card.clone(),
                grant.clone(),
                review_fact.clone(),
                self.semantic_snapshot(agent, "tool.request", call)?,
            ],
        );
        attempt.refs.extend(consumed.map(str::to_string));
        let contracts = vec![
            projected(&attempt, "bound_tool_grant", grant_fits, vec![card, grant]),
            projected(&attempt, "review_gate", review_fits, vec![review_fact]),
        ];
        self.record_tool_contract(
            agent,
            &attempt,
            ToolBoundary::Invoke,
            &contracts,
            legacy_allowed,
            gate,
        )
    }

    /// Observe approval eligibility without changing the legacy ceiling.
    pub(super) fn record_review_ceiling(
        &self,
        child: &Agent,
        approver: &Agent,
        operation: &str,
        request: &ReviewRequest,
        allowed: bool,
    ) -> Result<(), ControlError> {
        let child_tool = child.tools().iter().find(|tool| tool.name() == operation);
        let parent_tool = approver
            .tools()
            .iter()
            .find(|tool| tool.name() == operation);
        let child_tag = child_tool.map(|tool| tag_of(Some(tool.as_ref())));
        let parent_tag = parent_tool.map(|tool| tag_of(Some(tool.as_ref())));
        let child_fits = child_tag
            .as_ref()
            .is_some_and(|tag| tag.permit == Permit::Ask);
        let parent_fits = parent_tag
            .as_ref()
            .is_some_and(|tag| tag.permit == Permit::Go);
        let fits = match (&child_tag, &parent_tag) {
            (Some(child), Some(parent)) => {
                let file_fits = child.file == parent.file
                    || matches!(child.file, FileFacet::None)
                    || matches!(parent.file, FileFacet::Unbounded);
                file_fits && Charter::new(parent.clone()).allows(child)
            }
            _ => false,
        };
        let child_evidence = self.semantic_snapshot(
            child,
            "review.child_grant",
            &json!({"card": child.card_hash(), "tag": child_tag}),
        )?;
        let parent_evidence = self.semantic_snapshot(
            child,
            "review.supervisor_grant",
            &json!({"principal": approver.id(), "card": approver.card_hash(), "tag": parent_tag}),
        )?;
        let scope = self.semantic_snapshot(child, "review.scope", &request.scope)?;
        let mut attempt = Attempt::new(
            format!("review-check:{}", crate::det_seam::Entropy::os().uuid_v4()),
            child.id().to_string(),
            Capability::exact(
                Polarity::Effect,
                "review.approve_once",
                Resource::new("review", [request.id.as_str(), operation]),
            ),
            vec![child_evidence.clone(), parent_evidence.clone(), scope],
        );
        attempt.refs.push(request.tape_id.clone());
        let contracts = vec![
            projected(
                &attempt,
                "child_ask_grant",
                child_fits,
                vec![child_evidence.clone()],
            ),
            projected(
                &attempt,
                "supervisor_go_grant",
                parent_fits,
                vec![parent_evidence.clone()],
            ),
            projected(
                &attempt,
                "charter_fit",
                fits,
                vec![child_evidence, parent_evidence],
            ),
        ];
        self.record_tool_contract(
            child,
            &attempt,
            ToolBoundary::ReviewCeiling,
            &contracts,
            allowed,
            "approval_eligibility",
        )
    }

    fn record_tool_contract(
        &self,
        agent: &Agent,
        attempt: &Attempt,
        boundary: ToolBoundary,
        contracts: &[ContractClaim],
        allowed: bool,
        gate: &str,
    ) -> Result<(), ControlError> {
        let profile = boundary.profile();
        let assessment = assess_contract(attempt, &profile, contracts);
        let comparison = match assessment.disposition {
            AssessmentDisposition::Unknown => "incomplete",
            AssessmentDisposition::Allow if allowed => "agree",
            AssessmentDisposition::Deny if !allowed => "agree",
            _ => "different",
        };
        self.append_audit_event(agent, Kind::Observation, "tool_contract", json!({
            "attempt": attempt, "gate": gate,
            "legacy": if allowed { "allow" } else { "deny" },
            "contract": {"mode": "shadow", "profile": profile, "claims": contracts, "assessment": assessment, "comparison": comparison},
        }).to_string(), attempt.refs.clone())?;
        Ok(())
    }
}

fn projected(
    attempt: &Attempt,
    role: &str,
    allowed: bool,
    evidence: Vec<FactRef>,
) -> ContractClaim {
    ContractClaim {
        role: role.into(),
        claim: Claim {
            subject: attempt.subject.clone(),
            capabilities: if allowed {
                vec![attempt.capability.clone()]
            } else {
                vec![]
            },
        },
        evidence,
    }
}
