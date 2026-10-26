//! Agent bodies: a mounted spool that thinks. A plain [`SpoolBeat`] is a
//! pure function event → reply; an [`AgentBody`] instead yields *steps*
//! and core drives them — effect-as-data on the delivery path. Every
//! model call and tool call the body asks for is admitted against the
//! thread's card and taped by core, so a persona beat never hides cost
//! inside a single clean-looking note: each step is its own anchored
//! event. What crosses a boundary in one step is byte-capped: clips,
//! env, and utterances are cut with a marker, and the original stays
//! addressable (the clip id on tape, the env's content hash).
//!
//! The model client is the host's, injected like the hook runner. A body
//! that yields `InvokeModel` with no client installed fails closed.
//! What the model sees is composed on the thread side
//! (`crate::agent::context`) from card, workspace, and host ingredients —
//! control gathers them and tapes the call; it does not compose.
//! The tape is a flight recorder, thin by contract: a model response is
//! stored as a content-addressed blob in the session, and the observation
//! records the hash, the size, and the provider's usage — never the full
//! payload. A failed call tapes a terminal observation citing its action,
//! so the tape never holds an invoke that cannot be told apart from one
//! still in flight. `InvokeTool` is refused in v1: tool acts are async
//! and this driver is deliberately synchronous, like the policy sort —
//! the async tool step lands with the thread-loop driver.

use std::sync::Arc;

use an_agent_semantic::{
    Attempt, BoundaryProfile, Capability, Claim, ContractClaim, Dependency, DependencyUse,
    Disposition, FactRef, Freshness, Outcome, Polarity, Receipt, Resource as SemanticResource,
    decide,
};
use serde_json::{Value, json};

use super::model_boundary::{ModelPublication, ModelRecord};
use super::semantic::{Admission, AuthorizedAttempt};
use super::{AgentControl, ControlError, DeliveryOutcome, SpoolDeclaration};
use crate::agent::Agent;
use crate::agent::context::{self, Clip, Envelope, SystemFacts};
use crate::memstream::Kind;
use crate::principal::card::ModelSpec;
use crate::workspace::WorkspaceRecord;

/// Re-exported: the message type the composition produces and the client
/// consumes. Its home is the thread side (`agent::context`).
pub use crate::agent::context::ModelMessage;

/// Hard cap on driven steps per beat. A runaway body burns at most this
/// many model calls, then fails like any failed spool.
const MAX_STEPS: usize = 8;

/// How many recent tape events the projection carries.
const PROJECTION_CLIPS: usize = 20;

/// Byte fuses: what may cross one boundary in one step. A clip entering a
/// projection or a model envelope, the workspace env entering an envelope,
/// an utterance leaving for the tape and the host. A fuse caps bytes and
/// marks the cut; the original stays addressable — a clip by its tape id,
/// the env by the content hash the action already cites.
const MAX_CLIP_BYTES: usize = 4 * 1024;
const MAX_ENV_BYTES: usize = 16 * 1024;
const MAX_UTTER_BYTES: usize = 16 * 1024;

/// Cut `text` at `max` (on a char boundary) and append `marker`. Under
/// the cap the text passes through untouched.
fn fuse(text: &str, max: usize, marker: &str) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{}", &text[..end], marker)
}

/// A clip over the cap is cut and marked with its full size; the clip id
/// in the projection (or the action's cite list) still names the original.
fn fuse_clip(text: &str) -> String {
    fuse(
        text,
        MAX_CLIP_BYTES,
        &format!("\n…[clip truncated, {} bytes total]", text.len()),
    )
}

/// An env over the cap is cut and marked with the content hash of the
/// full text — the same hash the invoke action cites. An env without a
/// hash on record falls back to the plain size marker.
fn fuse_env(text: &str, sha256: Option<&str>) -> String {
    let marker = match sha256 {
        Some(sha) => format!("\n…[env truncated, full text sha256:{sha}]"),
        None => format!("\n…[env truncated, {} bytes total]", text.len()),
    };
    fuse(text, MAX_ENV_BYTES, &marker)
}

/// An utterance over the cap is cut and marked. Tape and host see the
/// same cut text — the fuse sits before both.
fn fuse_utter(text: &str) -> String {
    fuse(
        text,
        MAX_UTTER_BYTES,
        &format!("\n…[utterance truncated, {} bytes total]", text.len()),
    )
}

/// Provider-reported token usage. `None` when the provider does not say
/// — the tape records the absence, it does not invent numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// What the model returned. The content goes to the session's blob
/// store; only its hash, size, and usage reach the tape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub content: String,
    pub usage: Option<Usage>,
}

/// The host's model client. The thread composes and core tapes; the
/// client only carries bytes to the model and back. Sync like the policy
/// sort — an async client wraps its own runtime.
pub trait ModelClient: Send + Sync {
    fn complete(&self, spec: &ModelSpec, messages: Vec<ModelMessage>)
    -> Result<Completion, String>;
}

/// One step an agent body yields. `Approve` from the thread-loop
/// continuation vocabulary is absent on purpose: model-proposed tool
/// calls belong to the thread loop, not the delivery path.
#[derive(Debug, Clone, PartialEq)]
pub enum BeatStep {
    /// Silence: the body chooses not to speak. Taped, nothing returned.
    Halt,
    /// The beat's reply — taped as the spool note, handed to the host.
    Utter { text: String },
    /// Ask core for one model completion over these tape clip ids.
    InvokeModel { clips: Vec<String> },
    /// Refused in v1 — tool acts are async, this driver is sync.
    InvokeTool { name: String, args: Value },
    /// A cooperative high-risk checkpoint. This is a request, not an
    /// authorization or a cancellation command to the semantic plane.
    RequestReview { reason: String },
}

/// A mounted body made of steps. Core hands it a projection (the
/// triggering event, recent tape clips, the step count) and drives what
/// comes back. The body never touches the model client, a tool, or the
/// tape.
pub trait AgentBody: Send + Sync {
    fn evaluate(&self, projection: &Value) -> Result<BeatStep, String>;
}

/// What the body sees. `clips` are the newest tape events, oldest first;
/// model observations from this beat join the tail as they land, so the
/// next step sees the model's answer. The tape is thin: an invoke
/// observation stores a content address, and the projection resolves it
/// back to text for the body — resolution is a read path, the WAL never
/// holds the payload.
fn projection(agent: &Agent, event: &WorkspaceRecord, steps: usize) -> Result<Value, ControlError> {
    let tape = agent.session().tape().read_all()?;
    let clips: Vec<Value> = tape
        .iter()
        .rev()
        // Audit records stay on the tape but do not consume the body's
        // context window or become an implicit model input.
        .filter(|event| {
            !event
                .tags
                .iter()
                .any(|tag| tag == "semantic" || tag == "audit")
        })
        .take(PROJECTION_CLIPS)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|e| {
            json!({
                "id": e.id,
                "tag": e.tags.first().cloned().unwrap_or_default(),
                "content": fuse_clip(&resolve_payload(agent, e)),
            })
        })
        .collect();
    Ok(json!({
        "event": { "id": event.id, "name": event.name, "body": event.body },
        "clips": clips,
        "steps": steps,
    }))
}

/// Resolve a thin observation's content address back to its text. A
/// missing or unreadable blob leaves the thin line as it is — the tape's
/// hash and size stay truthful, and the gap is visible instead of
/// invented content.
fn resolve_payload(agent: &Agent, event: &crate::memstream::Memevent) -> String {
    if event.kind != Kind::Observation {
        return event.content.clone();
    }
    let Ok(content) = serde_json::from_str::<Value>(&event.content) else {
        return event.content.clone();
    };
    let Some(sha) = content.get("response_sha256").and_then(|s| s.as_str()) else {
        return event.content.clone();
    };
    agent
        .session()
        .read_blob(sha)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_else(|| event.content.clone())
}

impl AgentControl {
    /// Install the host's model client. A body yielding `InvokeModel`
    /// with no client fails closed (`ModelClientMissing`).
    pub fn set_model_client(&self, client: Arc<dyn ModelClient>) {
        let mut slot = self
            .inner
            .model_client
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        *slot = Some(client);
    }

    /// The embedding software's own label ("dsh-shuttle", "canvas-app").
    /// Stated in the model envelope's host facts. Defaults to `an-agent`.
    pub fn set_host_label(&self, label: &str) {
        let mut slot = self
            .inner
            .host_label
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        *slot = label.to_string();
    }

    fn model_client(&self) -> Result<Arc<dyn ModelClient>, ControlError> {
        self.inner
            .model_client
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
            .ok_or(ControlError::ModelClientMissing)
    }

    /// Drive one agent body through its steps. A halt settles as
    /// [`DeliveryOutcome::Halt`] carrying its silence note's tape id — a
    /// taped, explicit silence, never a synthesized empty reply. Any body
    /// error fails like a spool failure: the note is taped, the body
    /// unmounts.
    pub(super) fn agent_invoke(
        &self,
        agent: &Agent,
        name: &str,
        body: &Arc<dyn AgentBody>,
        event: &WorkspaceRecord,
        declaration: &SpoolDeclaration,
        delivery: &AuthorizedAttempt,
    ) -> Result<Result<DeliveryOutcome, String>, ControlError> {
        for steps in 0..MAX_STEPS {
            self.ensure_delivery_current(agent, &delivery.grant.facts)?;
            let step = match body.evaluate(&projection(agent, event, steps)?) {
                Ok(step) => step,
                Err(reason) => return Ok(Err(reason)),
            };
            self.ensure_delivery_current(agent, &delivery.grant.facts)?;
            match step {
                BeatStep::Utter { text } => {
                    let text = fuse_utter(&text);
                    return Ok(Ok(self.publish_reply(
                        agent,
                        name,
                        &event.id,
                        declaration,
                        text,
                        &delivery.grant.facts,
                    )?));
                }

                BeatStep::Halt => {
                    let tape_id =
                        self.with_delivery_current(agent, &delivery.grant.facts, || {
                            self.note_spool(agent, name, &event.id, Ok(""))
                        })?;
                    return Ok(Ok(DeliveryOutcome::Halt {
                        spool: name.to_string(),
                        event_id: event.id.clone(),
                        tape_id,
                    }));
                }
                BeatStep::InvokeModel { clips } => {
                    if let Some(request) =
                        self.model_step(agent, name, event, &clips, declaration, delivery)?
                    {
                        return Ok(Ok(DeliveryOutcome::NeedsReview(Box::new(request))));
                    }
                }
                BeatStep::RequestReview { reason } => {
                    let mut attempt = Attempt::new(
                        format!("review:{}", crate::det_seam::Entropy::os().uuid_v4()),
                        name,
                        Capability::exact(
                            Polarity::Effect,
                            "body.continue",
                            SemanticResource::new("spool", [name]),
                        ),
                        delivery.grant.facts.clone(),
                    );
                    attempt.refs = vec![delivery.tape_id.clone()];
                    let request = self.request_review(
                        agent,
                        name,
                        Some(&event.id),
                        attempt,
                        super::ReviewCause::HighRisk {
                            reason: fuse_utter(&reason),
                        },
                        None,
                    )?;
                    return Ok(Ok(DeliveryOutcome::NeedsReview(Box::new(request))));
                }
                BeatStep::InvokeTool { name: tool, .. } => {
                    return Ok(Err(format!(
                        "invoke_tool ({tool}) is not admitted in agent beats yet"
                    )));
                }
            }
        }
        Ok(Err(format!(
            "agent body did not conclude within {MAX_STEPS} steps"
        )))
    }

    /// One admitted, taped model call. Control gathers the workspace
    /// ingredients and the host facts; the thread side composes the
    /// envelope (`agent::context`). The action anchors the clips it read
    /// and cites the env/config it saw by content address; the
    /// observation anchors the action. The response joins the projection
    /// for the body's next step.
    fn model_step(
        &self,
        agent: &Agent,
        name: &str,
        event: &WorkspaceRecord,
        clips: &[String],
        declaration: &SpoolDeclaration,
        delivery: &AuthorizedAttempt,
    ) -> Result<Option<super::ReviewRequest>, ControlError> {
        let capability = Capability::exact(
            Polarity::Effect,
            "model.complete",
            SemanticResource::new(
                "model.endpoint",
                [agent.model().base_url.clone(), agent.model().model.clone()],
            ),
        );
        let claims = [
            Claim {
                subject: name.into(),
                capabilities: match declaration.model {
                    crate::act::ModelFacet::None => Vec::new(),
                    crate::act::ModelFacet::Complete => vec![capability.clone()],
                },
            },
            Claim {
                subject: name.into(),
                capabilities: vec![capability.clone()],
            },
        ];
        let mut facts = delivery.grant.facts.clone();
        facts.push(FactRef::new("agent.card", agent.card_hash()));
        let mut attempt = Attempt::new(
            format!("model:{}", crate::det_seam::Entropy::os().uuid_v4()),
            name,
            capability,
            facts,
        );
        attempt.refs = clips.to_vec();
        attempt.refs.push(delivery.tape_id.clone());
        let profile = BoundaryProfile {
            id: "core.model.v1".into(),
            required_roles: vec!["mounted_model_face".into(), "configured_endpoint".into()],
            checked: vec![Capability {
                polarity: Polarity::Effect,
                operation: "model.complete".into(),
                resource: SemanticResource::new("model.endpoint", std::iter::empty::<&str>()),
                recursive: true,
            }],
            limitations: vec![
                "Checks the declared model face and card-configured endpoint, not provider behavior.".into(),
                "The configured endpoint is not an independent host egress grant; the native client remains trusted.".into(),
            ],
        };
        let contracts = [
            ContractClaim {
                role: "mounted_model_face".into(),
                claim: claims[0].clone(),
                evidence: attempt
                    .facts
                    .iter()
                    .filter(|f| f.kind == "spool.declaration")
                    .cloned()
                    .collect(),
            },
            ContractClaim {
                role: "configured_endpoint".into(),
                claim: claims[1].clone(),
                evidence: attempt
                    .facts
                    .iter()
                    .filter(|f| f.kind == "agent.card")
                    .cloned()
                    .collect(),
            },
        ];
        // Preflight catches a missing model face before reading clips or
        // requiring a client. The execution gate below binds the request.
        if decide(&attempt, &claims).disposition == Disposition::Deny
            && let Admission::Denied {
                mut attempt,
                tape_id,
                reason,
            } = self.authorize_attempt(agent, &attempt, &claims, &profile, &contracts)?
        {
            attempt.refs.push(tape_id);
            return self
                .request_review(
                    agent,
                    name,
                    Some(&event.id),
                    attempt,
                    super::ReviewCause::PermissionDenied { reason },
                    None,
                )
                .map(Some);
        }
        let client = self.model_client()?;
        let tape = agent.session().tape().read_all()?;
        let mut resolved = Vec::with_capacity(clips.len());
        for clip in clips {
            match tape.iter().find(|e| &e.id == clip) {
                Some(e) if e.tags.iter().any(|tag| tag == "audit" || tag == "semantic") => {
                    return Err(ControlError::IneligibleClip(clip.clone()));
                }
                Some(e) => resolved.push((
                    e.tags.first().cloned().unwrap_or_else(|| "event".into()),
                    fuse_clip(&resolve_payload(agent, e)),
                )),
                None => return Err(ControlError::UnknownClip(clip.clone())),
            }
        }
        let views: Vec<Clip> = resolved
            .iter()
            .map(|(tag, content)| Clip { tag, content })
            .collect();
        let (cite, mut dependencies) = self.model_dependencies(agent, &delivery.grant.facts)?;
        let (guidance, enforced) = match self.config_at_revision(cite.config_sha256.as_deref())? {
            Some(config) => (
                config.guidance.rules,
                config
                    .guard
                    .iter()
                    .map(|guard| guard.rule.clone())
                    .collect(),
            ),
            None => (Vec::new(), Vec::new()),
        };
        let env = fuse_env(&cite.env_markdown, cite.env_sha256.as_deref());
        let messages = context::compose(&Envelope {
            prompt: agent.prompt(),
            facts: &self.system_facts(),
            env: &env,
            enforced: &enforced,
            guidance: &guidance,
            clips: &views,
        });
        // Exact application request, including the composed messages and
        // model knobs. Credentials remain solely inside the host client.
        let request = self.semantic_snapshot(
            agent,
            "model.request",
            &json!({
                "model": agent.model(),
                "messages": messages,
            }),
        )?;
        if let Some(sha) = &cite.config_sha256 {
            attempt.facts.push(FactRef::new("workspace.config", sha));
        }
        if let Some(sha) = &cite.env_sha256 {
            attempt.facts.push(FactRef::new("workspace.env", sha));
        }
        attempt.facts.push(request.clone());
        for dependency in &dependencies.dependencies {
            if !attempt.facts.contains(&dependency.fact) {
                attempt.facts.push(dependency.fact.clone());
            }
        }
        // Explicit clips are pinned history, not a demand that the workspace
        // still match the inputs that originally produced each clip.
        for clip in clips {
            let fact = FactRef::new("tape.clip", clip);
            if !dependencies.dependencies.iter().any(|d| d.fact == fact) {
                attempt.facts.push(fact.clone());
                dependencies.dependencies.push(Dependency {
                    fact,
                    usage: DependencyUse::ContextEvidence,
                    freshness: Freshness::Pinned,
                });
            }
        }
        dependencies.dependencies.push(Dependency {
            fact: request.clone(),
            usage: DependencyUse::DerivedArtifact,
            freshness: Freshness::Pinned,
        });
        let authorization =
            match self.authorize_attempt(agent, &attempt, &claims, &profile, &contracts)? {
                Admission::Allowed(authorization) => authorization,
                Admission::Denied {
                    mut attempt,
                    tape_id,
                    reason,
                } => {
                    attempt.refs.push(tape_id);
                    return self
                        .request_review(
                            agent,
                            name,
                            Some(&event.id),
                            attempt,
                            super::ReviewCause::PermissionDenied { reason },
                            None,
                        )
                        .map(Some);
                }
            };
        let mut refs = clips.to_vec();
        refs.push(authorization.tape_id.clone());
        let action = match self.publish_model_record(
            agent,
            &attempt,
            &dependencies,
            ModelRecord {
                kind: Kind::Action,
                content: json!({
                    "spool": name,
                    "event": event.id,
                    "model": agent.model().model,
                    "clips": clips,
                    "env": cite.env_sha256,
                    "config": cite.config_sha256,
                    "request_sha256": request.revision,
                    "semantic": authorization.tape_id,
                }),
                refs,
            },
        )? {
            ModelPublication::Published(action) => action,
            ModelPublication::Quarantined(request) => return Ok(Some(*request)),
        };
        let completion = match client.complete(agent.model(), messages) {
            Ok(completion) => completion,
            Err(reason) => {
                let receipt = Receipt::new(
                    authorization.grant,
                    Outcome::Unknown,
                    Vec::new(),
                    vec![authorization.tape_id, action.clone()],
                );
                // A failed call is not a dangling action: the terminal
                // observation closes it, citing the action it answers.
                let publication = self.publish_model_record(
                    agent,
                    &attempt,
                    &dependencies,
                    ModelRecord { kind: Kind::Observation,
                        content: json!({ "spool": name, "event": event.id, "status": "failed", "reason": reason, "receipt": receipt }),
                        refs: vec![action] },
                )?;
                if let ModelPublication::Quarantined(request) = publication {
                    return Ok(Some(*request));
                }
                return Err(ControlError::ModelFailed {
                    spool: name.into(),
                    reason,
                });
            }
        };
        let sha = agent.session().put_blob(completion.content.as_bytes())?;
        let receipt = Receipt::new(
            authorization.grant,
            Outcome::Succeeded,
            vec![FactRef::new("model.response", &sha)],
            vec![authorization.tape_id, action.clone()],
        );
        let publication = self.publish_model_record(
            agent,
            &attempt,
            &dependencies,
            ModelRecord {
                kind: Kind::Observation,
                content: json!({
                    "spool": name,
                    "event": event.id,
                    "response_sha256": sha,
                    "response_bytes": completion.content.len(),
                    "usage": completion.usage,
                    "receipt": receipt,
                }),
                refs: vec![action],
            },
        )?;
        Ok(match publication {
            ModelPublication::Published(_) => None,
            ModelPublication::Quarantined(request) => Some(*request),
        })
    }

    /// The host label plus platform facts, composed fresh each call.
    fn system_facts(&self) -> SystemFacts {
        let label = self
            .inner
            .host_label
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        SystemFacts::new(label)
    }
}
