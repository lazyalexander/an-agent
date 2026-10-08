//! Agent bodies: a mounted spool that thinks. A plain [`SpoolBeat`] is a
//! pure function event → reply; an [`AgentBody`] instead yields *steps*
//! and core drives them — effect-as-data on the delivery path. Every
//! model call and tool call the body asks for is admitted against the
//! thread's card and taped by core, so a persona beat never hides cost
//! inside a single clean-looking note: each step is its own anchored
//! event.
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

use serde_json::{Value, json};

use super::{AgentControl, ControlError, DeliveryOutcome, SpoolReply};
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
        .take(PROJECTION_CLIPS)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|e| {
            json!({
                "id": e.id,
                "tag": e.tags.first().cloned().unwrap_or_default(),
                "content": resolve_payload(agent, e),
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
    ) -> Result<Result<DeliveryOutcome, String>, ControlError> {
        let declaration = self.declaration_of(name)?;
        for steps in 0..MAX_STEPS {
            let step = match body.evaluate(&projection(agent, event, steps)?) {
                Ok(step) => step,
                Err(reason) => return Ok(Err(reason)),
            };
            match step {
                BeatStep::Utter { text } => {
                    let tape_id = self.note_spool(agent, name, &event.id, Ok(text.as_str()))?;
                    let intents = self.admit_intents(
                        agent,
                        name,
                        &event.id,
                        &declaration,
                        super::spool::parse_intents(&text),
                    )?;
                    return Ok(Ok(DeliveryOutcome::Reply(SpoolReply {
                        spool: name.to_string(),
                        event_id: event.id.clone(),
                        reply: text,
                        tape_id,
                        intents,
                    })));
                }
                BeatStep::Halt => {
                    let tape_id = self.note_spool(agent, name, &event.id, Ok(""))?;
                    return Ok(Ok(DeliveryOutcome::Halt {
                        spool: name.to_string(),
                        event_id: event.id.clone(),
                        tape_id,
                    }));
                }
                BeatStep::InvokeModel { clips } => {
                    // The mount's model face is the grant: undeclared
                    // means refused, like file/net/proc.
                    if declaration.model == crate::act::ModelFacet::None {
                        return Ok(Err(format!(
                            "invoke_model is not in {name}'s mounted model face"
                        )));
                    }
                    self.model_step(agent, name, event, &clips)?;
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
    ) -> Result<(), ControlError> {
        let client = self.model_client()?;
        let tape = agent.session().tape().read_all()?;
        let mut resolved = Vec::with_capacity(clips.len());
        for clip in clips {
            match tape.iter().find(|e| &e.id == clip) {
                Some(e) => resolved.push((
                    e.tags.first().cloned().unwrap_or_else(|| "event".into()),
                    e.content.clone(),
                )),
                None => return Err(ControlError::UnknownClip(clip.clone())),
            }
        }
        let views: Vec<Clip> = resolved
            .iter()
            .map(|(tag, content)| Clip { tag, content })
            .collect();
        let cite = self.workspace_cite()?;
        let guidance = self
            .current_config()?
            .map(|config| config.guidance.rules)
            .unwrap_or_default();
        let messages = context::compose(&Envelope {
            prompt: agent.prompt(),
            facts: &self.system_facts(),
            env: &cite.env_markdown,
            guidance: &guidance,
            clips: &views,
        });
        let action = self.append_event(
            agent,
            Kind::Action,
            "invoke",
            json!({
                "spool": name,
                "event": event.id,
                "model": agent.model().model,
                "clips": clips,
                "env": cite.env_sha256,
                "config": cite.config_sha256,
            })
            .to_string(),
            clips.to_vec(),
        )?;
        let completion = match client.complete(agent.model(), messages) {
            Ok(completion) => completion,
            Err(reason) => {
                // A failed call is not a dangling action: the terminal
                // observation closes it, citing the action it answers.
                self.append_event(
                    agent,
                    Kind::Observation,
                    "invoke",
                    json!({ "spool": name, "status": "failed", "reason": reason }).to_string(),
                    vec![action],
                )?;
                return Err(ControlError::ModelFailed {
                    spool: name.into(),
                    reason,
                });
            }
        };
        let sha = agent.session().put_blob(completion.content.as_bytes())?;
        self.append_event(
            agent,
            Kind::Observation,
            "invoke",
            json!({
                "spool": name,
                "response_sha256": sha,
                "response_bytes": completion.content.len(),
                "usage": completion.usage,
            })
            .to_string(),
            vec![action],
        )?;
        Ok(())
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
