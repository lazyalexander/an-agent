//! Mounted bodies and delivery. A body receives one workspace event.
//! It does not read the log, write the tape, or store config or env.
//! The host lands a reply in software; [`AgentControl::applied`] records
//! that landing on the thread tape.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use super::agent_beat::AgentBody;
use super::{AgentControl, ControlError};
use crate::agent::Agent;
use crate::workspace::WorkspaceRecord;

/// A body that receives one workspace event from [`AgentControl::offer`]
/// or [`AgentControl::dispatch`]. It does not read the log, write the tape,
/// or call another body.
pub trait SpoolBeat: Send + Sync {
    fn receive(&self, event: &WorkspaceRecord) -> Result<String, String>;
}

/// One intent a reply asks the host to execute. A spool has no initiation
/// channel of its own — it expresses, the host decides whether and when to
/// execute. The after hook chain is the gate before execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReplyIntent {
    /// Say this to the user — what "say" means is the host's business.
    Utter { text: String },
    /// Ask the host to push a workspace event. Admitted only when the
    /// event is in the spool's declared `produces`; anything else is
    /// stripped and the denial taped.
    Raise { event: String, body: String },
}

/// The wire convention: a reply string that parses *cleanly* as
/// `{"utter": "...", "raise": [{"event": "...", "body": "..."}]}` (either
/// key optional, no unknown keys) is structured intents. Anything else —
/// plain prose, other JSON, a near-miss with a mistyped field — is one
/// `Utter` carrying the raw string. The boundary is a full clean parse,
/// nothing looser, so an intent the kernel does not know yet can never
/// vanish silently: it degrades to visible text.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IntentsDoc {
    utter: Option<String>,
    raise: Option<Vec<RaiseDoc>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RaiseDoc {
    event: String,
    #[serde(default)]
    body: String,
}

pub(super) fn parse_intents(reply: &str) -> Vec<ReplyIntent> {
    let plain = || {
        vec![ReplyIntent::Utter {
            text: reply.to_string(),
        }]
    };
    let Ok(doc) = serde_json::from_str::<IntentsDoc>(reply) else {
        return plain();
    };
    if doc.utter.is_none() && doc.raise.is_none() {
        return plain();
    }
    let mut intents = Vec::new();
    if let Some(text) = doc.utter {
        intents.push(ReplyIntent::Utter { text });
    }
    for raise in doc.raise.unwrap_or_default() {
        intents.push(ReplyIntent::Raise {
            event: raise.event,
            body: raise.body,
        });
    }
    intents
}

/// What one offered body returned. The workspace event was already on the log.
/// `tape_id` is the spool note on the thread tape. The host cites it when
/// it records [`AgentControl::applied`]. `intents` is the admitted view of
/// what the reply asks for — always populated; a plain-text reply is one
/// `Utter`. The host iterates intents; `reply` stays the raw taped form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpoolReply {
    pub spool: String,
    pub event_id: String,
    pub reply: String,
    pub tape_id: String,
    pub intents: Vec<ReplyIntent>,
}

/// What one delivered event settled as for one body. A halt is a taped,
/// explicit silence — it carries its own spool note id and nothing for
/// the host to land, so it never earns an `applied`. A reply carries
/// admitted intents for the host to land. The two are never collapsed
/// into one shape: an empty utterance is a reply, a halt is not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryOutcome {
    Halt {
        spool: String,
        event_id: String,
        tape_id: String,
    },
    Reply(SpoolReply),
}

impl DeliveryOutcome {
    /// The reply, when the body spoke.
    pub fn reply(&self) -> Option<&SpoolReply> {
        match self {
            Self::Reply(reply) => Some(reply),
            Self::Halt { .. } => None,
        }
    }
}

/// A mounted body and the declaration it was matched with. The declaration
/// stays: `produces` gates each reply's raise intents at delivery, not
/// only at mount.
pub(super) struct Mounted {
    pub name: String,
    pub body: MountedBody,
    pub declaration: SpoolDeclaration,
}

/// The two sorts of mounted body. A `Beat` is a pure function event →
/// reply; an `Agent` yields steps and core drives them (see
/// `agent_beat`). Mount, matching, and delivery rules are shared.
#[derive(Clone)]
pub(super) enum MountedBody {
    Beat(Arc<dyn SpoolBeat>),
    Agent(Arc<dyn AgentBody>),
}

/// A mounted body's claim about workspace events: which it receives
/// (`consumes`) and which it may emit (`produces`). Matched against the
/// register at mount — the bind point — and kept afterwards: `produces`
/// is also the whitelist that gates each reply's raise intents at
/// delivery. Matching at mount is mutual, both directions:
///
/// - every consumed event must be registered *and* routed to this spool
///   (the workspace must be able to deliver what the spool asks for);
/// - every produced event must be registered (the workspace must admit
///   what the spool may emit);
/// - every route naming this spool must be in its `consumes` (the spool
///   must have asked for what the workspace promises to deliver).
///
/// Shape schemas are not matched yet: the workspace declares names only,
/// so there is nothing to compare a payload shape against.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpoolDeclaration {
    pub consumes: Vec<String>,
    pub produces: Vec<String>,
}

impl AgentControl {
    /// Mount a body under `name`. A second mount of the same name is refused.
    /// The declaration is matched against the current registration; a
    /// mismatch refuses the mount. With no registration at all, only a
    /// silent body (no consumes, no produces) mounts.
    pub fn mount_spool(
        &self,
        name: &str,
        body: Arc<dyn SpoolBeat>,
        declaration: &SpoolDeclaration,
    ) -> Result<(), ControlError> {
        self.match_declaration(name, declaration)?;
        let mut bodies = self
            .inner
            .bodies
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        if bodies.iter().any(|mounted| mounted.name == name) {
            return Err(ControlError::AlreadyMounted(name.to_string()));
        }
        bodies.push(Mounted {
            name: name.to_string(),
            body: MountedBody::Beat(body),
            declaration: declaration.clone(),
        });
        Ok(())
    }

    /// Mount an agent body under `name`. Same rules as [`Self::mount_spool`]:
    /// the declaration is matched at mount, a second mount of the name is
    /// refused. The body additionally needs a model client installed before
    /// it can ask for model calls.
    pub fn mount_agent(
        &self,
        name: &str,
        body: Arc<dyn AgentBody>,
        declaration: &SpoolDeclaration,
    ) -> Result<(), ControlError> {
        self.match_declaration(name, declaration)?;
        let mut bodies = self
            .inner
            .bodies
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        if bodies.iter().any(|mounted| mounted.name == name) {
            return Err(ControlError::AlreadyMounted(name.to_string()));
        }
        bodies.push(Mounted {
            name: name.to_string(),
            body: MountedBody::Agent(body),
            declaration: declaration.clone(),
        });
        Ok(())
    }

    /// The bind-time match. All mismatches are reported together, so one
    /// failed mount names every broken promise, not just the first.
    fn match_declaration(
        &self,
        name: &str,
        declaration: &SpoolDeclaration,
    ) -> Result<(), ControlError> {
        let registration = match self.current_registration() {
            Ok(registration) => registration,
            // No counterpart to match against: a silent body mounts, an
            // event-taking one reports the missing register.
            Err(ControlError::NotRegistered)
                if declaration.consumes.is_empty() && declaration.produces.is_empty() =>
            {
                return Ok(());
            }
            Err(err) => return Err(err),
        };
        let mut mismatches = Vec::new();
        for event in &declaration.consumes {
            match registration
                .events
                .iter()
                .find(|route| &route.name == event)
            {
                None => mismatches.push(format!("consumes unregistered event {event}")),
                Some(route) if !route.spools.iter().any(|spool| spool == name) => mismatches.push(
                    format!("consumes {event} but its route does not name {name}"),
                ),
                Some(_) => {}
            }
        }
        for event in &declaration.produces {
            if !registration.events.iter().any(|route| &route.name == event) {
                mismatches.push(format!("produces unregistered event {event}"));
            }
        }
        for route in registration
            .events
            .iter()
            .filter(|route| route.spools.iter().any(|spool| spool == name))
        {
            if !declaration.consumes.contains(&route.name) {
                mismatches.push(format!(
                    "route {} names {name} but it is not consumed",
                    route.name
                ));
            }
        }
        if mismatches.is_empty() {
            Ok(())
        } else {
            Err(ControlError::MountMismatch {
                name: name.to_string(),
                reasons: mismatches,
            })
        }
    }

    /// Remove a mounted body. The workspace log is left as it is.
    pub fn unmount_spool(&self, name: &str) -> Result<(), ControlError> {
        let mut bodies = self
            .inner
            .bodies
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let Some(pos) = bodies.iter().position(|mounted| mounted.name == name) else {
            return Err(ControlError::NotMounted(name.to_string()));
        };
        bodies.remove(pos);
        Ok(())
    }

    /// One beat. The named body receives the latest workspace event.
    pub fn offer(&self, id: Uuid, name: &str) -> Result<DeliveryOutcome, ControlError> {
        let event = self.latest_event()?;
        self.offer_on(id, name, &event.id)
    }

    /// One beat. The named body receives the workspace event `event_id`.
    /// Core writes the reply on the thread tape. The body does not.
    /// A failure unmounts that body only. The event stays on the log.
    /// The event passes the before chain; a reply passes the after chain.
    /// A halt passes neither — there is nothing to gate.
    pub fn offer_on(
        &self,
        id: Uuid,
        name: &str,
        event_id: &str,
    ) -> Result<DeliveryOutcome, ControlError> {
        self.enter_beat(id)?;
        let turn = self.inner.pool.begin_turn(id)?;
        let event = self.event_by_id(event_id)?;
        self.hooks_before(turn.agent(), &event)?;
        match self.invoke(turn.agent(), name, &event)? {
            Ok(outcome) => {
                if let DeliveryOutcome::Reply(reply) = &outcome {
                    self.hooks_after(turn.agent(), reply)?;
                }
                Ok(outcome)
            }
            Err(reason) => Err(ControlError::SpoolFailed {
                name: name.to_string(),
                reason,
            }),
        }
    }

    /// One beat. The latest event goes to each spool on its route, in order.
    pub fn dispatch(&self, id: Uuid) -> Result<Vec<DeliveryOutcome>, ControlError> {
        let event = self.latest_event()?;
        self.dispatch_on(id, &event.id)
    }

    /// One beat. The named event goes to each spool on its route, in order.
    /// A missing spool is noted and skipped. A failed spool is unmounted and
    /// the rest still run. Outcomes — replies and halts alike — are returned
    /// to the host. They are not written as new workspace events. The event
    /// passes the before chain once; each reply passes the after chain — a
    /// denied reply does not reach the host, and the denial is taped.
    pub fn dispatch_on(
        &self,
        id: Uuid,
        event_id: &str,
    ) -> Result<Vec<DeliveryOutcome>, ControlError> {
        self.enter_beat(id)?;
        let turn = self.inner.pool.begin_turn(id)?;
        let event = self.event_by_id(event_id)?;
        self.hooks_before(turn.agent(), &event)?;
        let event_name = event.name.clone().unwrap_or_default();
        let route = self.spools_for(&event_name)?;
        let mut outcomes = Vec::new();
        for spool in route {
            match self.invoke(turn.agent(), &spool, &event) {
                Ok(Ok(outcome)) => match &outcome {
                    DeliveryOutcome::Reply(reply) => match self.hooks_after(turn.agent(), reply) {
                        Ok(()) => outcomes.push(outcome),
                        Err(ControlError::HookDenied { .. }) => {}
                        Err(err) => return Err(err),
                    },
                    DeliveryOutcome::Halt { .. } => outcomes.push(outcome),
                },
                Ok(Err(_)) => {}
                Err(ControlError::NotMounted(_)) => {
                    self.note_spool(turn.agent(), &spool, &event.id, Err("missing"))?;
                }
                Err(err) => return Err(err),
            }
        }
        drop(turn);
        Ok(outcomes)
    }

    /// The host landed `reply` in software. Core writes that claim on the
    /// thread tape and cites the spool note. It does not mutate the host.
    /// Does not take the beat slot.
    pub fn applied(
        &self,
        id: Uuid,
        reply: &SpoolReply,
        note: &str,
    ) -> Result<String, ControlError> {
        let agent = self.live(id)?;
        let content = json!({
            "spool": reply.spool,
            "event": reply.event_id,
            "reply": reply.reply,
            "note": note,
        })
        .to_string();
        let refs = if reply.tape_id.is_empty() {
            Vec::new()
        } else {
            vec![reply.tape_id.clone()]
        };
        self.append_event(
            &agent,
            crate::memstream::Kind::Action,
            "applied",
            content,
            refs,
        )
    }

    /// Deliver one event to one mounted body. A body failure unmounts the
    /// body and is returned as `Err(reason)`, never as a `ControlError`.
    fn invoke(
        &self,
        agent: &Agent,
        name: &str,
        event: &WorkspaceRecord,
    ) -> Result<Result<DeliveryOutcome, String>, ControlError> {
        let Some((body, declaration)) = self.body_named(name) else {
            return Err(ControlError::NotMounted(name.to_string()));
        };
        match body {
            MountedBody::Agent(body) => match self.agent_invoke(agent, name, &body, event)? {
                Err(reason) => {
                    self.drop_body(name);
                    self.note_spool(agent, name, &event.id, Err(reason.as_str()))?;
                    Ok(Err(reason))
                }
                Ok(outcome) => Ok(Ok(outcome)),
            },
            MountedBody::Beat(body) => match body.receive(event) {
                Ok(reply) => {
                    let tape_id = self.note_spool(agent, name, &event.id, Ok(reply.as_str()))?;
                    let intents = self.admit_intents(
                        agent,
                        name,
                        &event.id,
                        &declaration,
                        parse_intents(&reply),
                    )?;
                    Ok(Ok(DeliveryOutcome::Reply(SpoolReply {
                        spool: name.to_string(),
                        event_id: event.id.clone(),
                        reply,
                        tape_id,
                        intents,
                    })))
                }
                Err(reason) => {
                    self.drop_body(name);
                    self.note_spool(agent, name, &event.id, Err(reason.as_str()))?;
                    Ok(Err(reason))
                }
            },
        }
    }

    /// The produces whitelist, applied per intent at delivery: an
    /// undeclared raise is stripped and the denial taped (tag `intent`),
    /// after the spool note so the tape reads ask-then-rule. The rest of
    /// the reply still travels — a spool that overreaches on one intent
    /// does not forfeit the ones it declared.
    pub(super) fn admit_intents(
        &self,
        agent: &Agent,
        spool: &str,
        event_id: &str,
        declaration: &SpoolDeclaration,
        intents: Vec<ReplyIntent>,
    ) -> Result<Vec<ReplyIntent>, ControlError> {
        let mut admitted = Vec::with_capacity(intents.len());
        for intent in intents {
            match &intent {
                ReplyIntent::Raise { event, .. } if !declaration.produces.contains(event) => {
                    let content = json!({
                        "spool": spool,
                        "event": event_id,
                        "verdict": "deny",
                        "intent": intent,
                        "reason": format!("raise of {event} is not in the mounted produces"),
                    });
                    self.append(agent, "intent", content.to_string())?;
                }
                _ => admitted.push(intent),
            }
        }
        Ok(admitted)
    }

    fn body_named(&self, name: &str) -> Option<(MountedBody, SpoolDeclaration)> {
        let bodies = self
            .inner
            .bodies
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        bodies
            .iter()
            .find(|mounted| mounted.name == name)
            .map(|mounted| (mounted.body.clone(), mounted.declaration.clone()))
    }

    /// The declaration a mounted body was matched with. Used by the agent
    /// driver, which admits intents itself.
    pub(super) fn declaration_of(&self, name: &str) -> Result<SpoolDeclaration, ControlError> {
        self.body_named(name)
            .map(|(_, declaration)| declaration)
            .ok_or_else(|| ControlError::NotMounted(name.to_string()))
    }

    fn drop_body(&self, name: &str) {
        let mut bodies = self
            .inner
            .bodies
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        bodies.retain(|mounted| mounted.name != name);
    }

    pub(super) fn note_spool(
        &self,
        agent: &Agent,
        spool: &str,
        event_id: &str,
        outcome: Result<&str, &str>,
    ) -> Result<String, ControlError> {
        let content = match outcome {
            Ok(reply) => json!({ "spool": spool, "event": event_id, "reply": reply }),
            Err(reason) => json!({ "spool": spool, "event": event_id, "error": reason }),
        };
        self.append(agent, "spool", content.to_string())
    }

    fn latest_event(&self) -> Result<WorkspaceRecord, ControlError> {
        self.inner
            .workspace
            .log()?
            .into_iter()
            .rev()
            .find(|record| record.kind == "event")
            .ok_or(ControlError::NoEvent)
    }

    fn event_by_id(&self, event_id: &str) -> Result<WorkspaceRecord, ControlError> {
        self.inner
            .workspace
            .log()?
            .into_iter()
            .find(|record| record.kind == "event" && record.id == event_id)
            .ok_or_else(|| ControlError::UnknownEvent(event_id.to_string()))
    }
}
