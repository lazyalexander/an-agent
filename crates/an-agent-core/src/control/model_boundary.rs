//! Model boundary adapter for the independent dependency checker. The control
//! lock serializes snapshots/publication with supported workspace and lifecycle
//! changes. It is never held across the external call and is not an OS fence.

use an_agent_semantic::{
    Attempt, Capability, Dependency, DependencyObservation, DependencyState, DependencyUse,
    FactRef, Freshness, Polarity, Resource, Validity, assess_dependencies,
};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{AgentControl, ControlError, ReviewCause, ReviewRequest, ReviewScope};
use crate::agent::Agent;
use crate::memstream::Kind;
use crate::workspace::WorkspaceCite;

pub(super) struct ModelDependencies {
    scope: ReviewScope,
    pub dependencies: Vec<Dependency>,
}

pub(super) enum ModelPublication {
    Published(String),
    Quarantined(Box<ReviewRequest>),
}

pub(super) struct ModelRecord {
    pub kind: Kind,
    pub content: Value,
    pub refs: Vec<String>,
}

impl AgentControl {
    pub(super) fn model_dependencies(
        &self,
        agent: &Agent,
        delivery: &[FactRef],
    ) -> Result<(WorkspaceCite, ModelDependencies), ControlError> {
        let state = self
            .inner
            .supervision
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mount = delivery
            .iter()
            .find(|f| f.kind == "spool.instance")
            .and_then(|f| Uuid::parse_str(&f.revision).ok())
            .ok_or_else(|| ControlError::ReviewStale("missing mount".into()))?;
        let scope = state.scope(agent.id(), Some(mount))?;
        if !delivery.iter().any(|f| {
            f.kind == "agent.instance" && f.revision == scope.child.incarnation.to_string()
        }) {
            return Err(ControlError::ReviewStale("retired agent instance".into()));
        }
        let cite = self.workspace_cite()?;
        let registration = delivery
            .iter()
            .find(|f| f.kind == "workspace.registration.generation")
            .cloned()
            .ok_or_else(|| ControlError::ReviewStale("missing registration generation".into()))?;
        let scope_fact = self.semantic_snapshot(agent, "control.scope", &scope)?;
        let dependencies = [
            (scope_fact, DependencyUse::Authorization),
            (registration, DependencyUse::Authorization),
            (
                FactRef::new(
                    "workspace.config.generation",
                    cite.config_id.as_deref().unwrap_or("absent"),
                ),
                DependencyUse::ExecutionPrecondition,
            ),
            (
                FactRef::new(
                    "workspace.env.generation",
                    cite.env_id.as_deref().unwrap_or("absent"),
                ),
                DependencyUse::ExecutionPrecondition,
            ),
        ]
        .into_iter()
        .map(|(fact, usage)| Dependency {
            fact,
            usage,
            freshness: Freshness::Current,
        })
        .collect();
        Ok((
            cite,
            ModelDependencies {
                scope,
                dependencies,
            },
        ))
    }

    /// Recheck at admission and at terminal publication. A synchronous provider
    /// may finish after revocation: quarantine records that fact, not a rollback.
    pub(super) fn publish_model_record(
        &self,
        agent: &Agent,
        attempt: &Attempt,
        dependencies: &ModelDependencies,
        mut record: ModelRecord,
    ) -> Result<ModelPublication, ControlError> {
        let mut state = self
            .inner
            .supervision
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let current = self.review_generations()?;
        let scope_current = state.current(&dependencies.scope);
        let tape = agent.session().tape().read_all()?;
        let observations: Vec<_> = dependencies
            .dependencies
            .iter()
            .map(|dependency| {
                let observed = if dependency.fact.kind == "control.scope" {
                    if scope_current {
                        DependencyState::Available {
                            current_revision: Some(dependency.fact.revision.clone()),
                        }
                    } else {
                        DependencyState::Withdrawn
                    }
                } else if dependency.fact.kind == "model.request" {
                    if agent.session().read_blob(&dependency.fact.revision).is_ok() {
                        DependencyState::Available {
                            current_revision: None,
                        }
                    } else {
                        DependencyState::Missing
                    }
                } else if dependency.fact.kind == "tape.clip" {
                    match tape
                        .iter()
                        .find(|event| event.id == dependency.fact.revision)
                    {
                        Some(event)
                            if event
                                .tags
                                .iter()
                                .any(|tag| tag == "audit" || tag == "semantic") =>
                        {
                            DependencyState::Withdrawn
                        }
                        Some(_) => DependencyState::Available {
                            current_revision: None,
                        },
                        None => DependencyState::Missing,
                    }
                } else {
                    // Workspace generations are append-only. "absent" is an
                    // explicit snapshot of absence, not a missing observation.
                    match current.iter().find(|f| f.kind == dependency.fact.kind) {
                        Some(fact) => DependencyState::Available {
                            current_revision: Some(fact.revision.clone()),
                        },
                        None => DependencyState::Missing,
                    }
                };
                DependencyObservation {
                    fact: dependency.fact.clone(),
                    state: observed,
                }
            })
            .collect();
        let assessment = assess_dependencies(&dependencies.dependencies, &observations);
        record.content["dependencies"] = json!(dependencies.dependencies);
        record.content["dependency_observations"] = json!(observations);
        record.content["dependency_assessment"] = json!(assessment);
        if assessment.validity == Validity::Valid {
            return self
                .append_event(
                    agent,
                    record.kind,
                    "invoke",
                    record.content.to_string(),
                    record.refs,
                )
                .map(ModelPublication::Published);
        }
        record.content["publication"] = json!("quarantined");
        record.content["phase"] = json!(if record.kind == Kind::Action {
            "before_call"
        } else {
            "after_call"
        });
        let event_id = record.content["event"].as_str().map(str::to_string);
        let evidence = self.append_audit_event(
            agent,
            Kind::Observation,
            "model_quarantined",
            record.content.to_string(),
            record.refs,
        )?;
        if !scope_current {
            // Do not create a request on a replacement mount, incarnation, or
            // authority epoch. The host gets the retained evidence id instead.
            return Err(ControlError::DependencyStale(evidence));
        }
        let mut facts = current;
        facts.push(FactRef::new(
            "agent.instance",
            dependencies.scope.child.incarnation.to_string(),
        ));
        if let Some(mount) = dependencies.scope.mount {
            facts.push(FactRef::new("spool.instance", mount.to_string()));
        }
        // This is a fresh scheduling request citing old evidence, not a new
        // grant for the stale model attempt. ApproveOnce cannot resolve it.
        let mut continuation = Attempt::new(
            format!(
                "dependency-review:{}",
                crate::det_seam::Entropy::os().uuid_v4()
            ),
            &attempt.subject,
            Capability::exact(
                Polarity::Effect,
                "body.continue",
                Resource::new("spool", [&attempt.subject]),
            ),
            facts,
        );
        continuation.refs.push(evidence.clone());
        let request = self.retain_review(
            &mut state,
            agent,
            &attempt.subject,
            event_id.as_deref(),
            continuation,
            ReviewCause::DependencyChanged { evidence },
            None,
        )?;
        Ok(ModelPublication::Quarantined(Box::new(request)))
    }
}
