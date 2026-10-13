//! Mounted bodies and delivery. A body receives one workspace event.
//! It does not read the log, write the tape, or store config or env.
//! The host lands a reply in software; [`AgentControl::applied`] records
//! that landing on the thread tape.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use an_agent_semantic::{
    Attempt, BoundaryProfile, Capability, Claim, ContractClaim, FactRef, Polarity,
    Resource as SemanticResource,
};

use super::agent_beat::AgentBody;
use super::semantic::Admission;
use super::workspace::RegistrationSnapshot;
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
/// admitted intents for the host to land. Review is a stopped operation,
/// not a halt, reply, or body failure; it has no applied landing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryOutcome {
    Halt {
        spool: String,
        event_id: String,
        tape_id: String,
    },
    Reply(SpoolReply),
    /// No effect or applied landing. The host routes the request and
    /// decides when to schedule fresh work after supervisor disposition.
    NeedsReview(Box<super::ReviewRequest>),
}

impl DeliveryOutcome {
    /// The reply, when the body spoke.
    pub fn reply(&self) -> Option<&SpoolReply> {
        match self {
            Self::Reply(reply) => Some(reply),
            Self::Halt { .. } | Self::NeedsReview(_) => None,
        }
    }
}

/// A mounted body and the declaration it was matched with. The declaration
/// stays: `produces` gates each reply's raise intents at delivery, not
/// only at mount.
#[derive(Clone)]
pub(super) struct Mounted {
    pub name: String,
    pub instance: Uuid,
    pub body: MountedBody,
    pub declaration: SpoolDeclaration,
    pub descriptor: Option<SpoolDescriptor>,
}

/// A stable package symbol and its mount-time YAML bytes. The trusted loader
/// validates the YAML and derives the body and declaration from it. Core
/// retains a copy, not a live file reference. This is not a code attestation,
/// version constraint, or session-owned implementation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpoolDescriptor {
    pub package: String,
    pub yaml: String,
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
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SpoolDeclaration {
    pub consumes: Vec<String>,
    pub produces: Vec<String>,
    /// Model-call ceiling. `None` (the default) refuses `invoke_model` —
    /// the face must be declared, like file/net/proc.
    pub model: crate::act::ModelFacet,
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
        self.mount_body(name, MountedBody::Beat(body), declaration, None)
    }

    /// Mount from a validated descriptor. Changing the descriptor requires
    /// an explicit unmount and a new mount; existing reviews never transfer.
    pub fn mount_spool_described(
        &self,
        name: &str,
        body: Arc<dyn SpoolBeat>,
        declaration: &SpoolDeclaration,
        descriptor: &SpoolDescriptor,
    ) -> Result<(), ControlError> {
        self.mount_body(name, MountedBody::Beat(body), declaration, Some(descriptor))
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
        self.mount_body(name, MountedBody::Agent(body), declaration, None)
    }

    /// The policy-body counterpart of [`Self::mount_spool_described`].
    pub fn mount_agent_described(
        &self,
        name: &str,
        body: Arc<dyn AgentBody>,
        declaration: &SpoolDeclaration,
        descriptor: &SpoolDescriptor,
    ) -> Result<(), ControlError> {
        self.mount_body(
            name,
            MountedBody::Agent(body),
            declaration,
            Some(descriptor),
        )
    }

    fn mount_body(
        &self,
        name: &str,
        body: MountedBody,
        declaration: &SpoolDeclaration,
        descriptor: Option<&SpoolDescriptor>,
    ) -> Result<(), ControlError> {
        if descriptor.is_some_and(|d| d.package.trim().is_empty() || d.yaml.trim().is_empty()) {
            return Err(ControlError::MountMismatch {
                name: name.into(),
                reasons: vec!["descriptor requires a package symbol and YAML bytes".into()],
            });
        }
        let mut state = self
            .inner
            .supervision
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        self.match_declaration(name, declaration)?;
        let mut bodies = self
            .inner
            .bodies
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        if bodies.iter().any(|mounted| mounted.name == name) {
            return Err(ControlError::AlreadyMounted(name.to_string()));
        }
        let instance = crate::det_seam::Entropy::os().uuid_v4();
        bodies.push(Mounted {
            name: name.to_string(),
            instance,
            body,
            declaration: declaration.clone(),
            descriptor: descriptor.cloned(),
        });
        state.mount(instance);
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
        let mut state = self
            .inner
            .supervision
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut bodies = self
            .inner
            .bodies
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let Some(pos) = bodies.iter().position(|mounted| mounted.name == name) else {
            return Err(ControlError::NotMounted(name.to_string()));
        };
        let removed = bodies.remove(pos);
        drop(bodies);
        let requests = state.unmount(removed.instance);
        self.record_review_closures(requests, "unmount")
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
        let turn = self.begin_beat(id)?;
        let event = self.event_by_id(event_id)?;
        self.hooks_before(turn.agent(), &event)?;
        let registration = self.current_registration_snapshot()?;
        match self.invoke(turn.agent(), name, &event, &registration)? {
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
        let turn = self.begin_beat(id)?;
        let event = self.event_by_id(event_id)?;
        self.hooks_before(turn.agent(), &event)?;
        let event_name = event.name.clone().unwrap_or_default();
        let (registration, route) = self.route_for(&event_name)?;
        let mut outcomes = Vec::new();
        for spool in route {
            match self.invoke(turn.agent(), &spool, &event, &registration) {
                Ok(Ok(outcome)) => match &outcome {
                    DeliveryOutcome::Reply(reply) => match self.hooks_after(turn.agent(), reply) {
                        Ok(()) => outcomes.push(outcome),
                        Err(ControlError::HookDenied { .. }) => {}
                        Err(err) => return Err(err),
                    },
                    DeliveryOutcome::Halt { .. } | DeliveryOutcome::NeedsReview(_) => {
                        outcomes.push(outcome)
                    }
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
        registration: &RegistrationSnapshot,
    ) -> Result<Result<DeliveryOutcome, String>, ControlError> {
        let Some(mounted) = self.body_named(name) else {
            return Err(ControlError::NotMounted(name.to_string()));
        };
        if let Some(request) = self.waiting_review(agent.id(), name, mounted.instance)? {
            return Ok(Ok(DeliveryOutcome::NeedsReview(Box::new(request))));
        }
        let delivery = match self.authorize_delivery(agent, name, event, registration, &mounted)? {
            Admission::Allowed(delivery) => delivery,
            Admission::Denied {
                mut attempt,
                tape_id,
                reason,
            } => {
                attempt.refs.push(tape_id);
                let request = self.request_review(
                    agent,
                    name,
                    Some(&event.id),
                    attempt,
                    super::ReviewCause::PermissionDenied { reason },
                    None,
                )?;
                return Ok(Ok(DeliveryOutcome::NeedsReview(Box::new(request))));
            }
        };
        self.ensure_delivery_current(agent, &delivery.grant.facts)?;
        let Mounted {
            body,
            declaration,
            instance,
            ..
        } = mounted;
        match body {
            MountedBody::Agent(body) => {
                match self.agent_invoke(agent, name, &body, event, &declaration, &delivery)? {
                    Err(reason) => {
                        self.drop_body(instance)?;
                        self.note_spool(agent, name, &event.id, Err(reason.as_str()))?;
                        Ok(Err(reason))
                    }
                    Ok(outcome) => Ok(Ok(outcome)),
                }
            }
            MountedBody::Beat(body) => match body.receive(event) {
                Ok(reply) => {
                    self.ensure_delivery_current(agent, &delivery.grant.facts)?;
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
                    self.drop_body(instance)?;
                    self.note_spool(agent, name, &event.id, Err(reason.as_str()))?;
                    Ok(Err(reason))
                }
            },
        }
    }

    /// Translate delivery into the semantic plane immediately before the
    /// body boundary. The mounted declaration and the registration snapshot
    /// are independent claims; both must cover the same concrete attempt.
    fn authorize_delivery(
        &self,
        agent: &Agent,
        name: &str,
        event: &WorkspaceRecord,
        registration: &RegistrationSnapshot,
        body: &Mounted,
    ) -> Result<Admission, ControlError> {
        let declaration = &body.declaration;
        let event_name = event.name.clone().unwrap_or_default();
        let capability = |event: &str| {
            Capability::exact(
                Polarity::Coeffect,
                "flow.consume",
                SemanticResource::new("workspace.event", [event]),
            )
        };
        let mounted = Claim {
            subject: name.to_string(),
            capabilities: declaration
                .consumes
                .iter()
                .map(|event| capability(event))
                .collect(),
        };
        let routed = registration
            .registration
            .events
            .iter()
            .find(|route| route.name == event_name)
            .is_some_and(|route| route.spools.iter().any(|spool| spool == name));
        let register = Claim {
            subject: name.to_string(),
            capabilities: if routed {
                vec![capability(&event_name)]
            } else {
                Vec::new()
            },
        };
        let mut attempt = Attempt::new(
            format!("delivery:{}", crate::det_seam::Entropy::os().uuid_v4()),
            name,
            capability(&event_name),
            vec![
                FactRef::new("workspace.registration", registration.revision.clone()),
                FactRef::new(
                    "workspace.registration.generation",
                    registration.generation.clone(),
                ),
                FactRef::new("spool.instance", body.instance.to_string()),
                FactRef::new(
                    "agent.instance",
                    self.inner
                        .supervision
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .instance(agent.id())?
                        .incarnation
                        .to_string(),
                ),
                self.semantic_snapshot(agent, "spool.declaration", declaration)?,
            ],
        );
        if let Some(descriptor) = &body.descriptor {
            attempt
                .facts
                .push(FactRef::new("spool.package", &descriptor.package));
            let digest = agent.session().put_blob(descriptor.yaml.as_bytes())?;
            attempt.facts.push(FactRef::new("spool.descriptor", digest));
        }
        attempt.refs.push(event.id.clone());
        let profile = BoundaryProfile {
            id: "core.delivery.v1".into(),
            required_roles: vec!["mounted_declaration".into(), "workspace_route".into()],
            checked: vec![Capability {
                polarity: Polarity::Coeffect,
                operation: "flow.consume".into(),
                resource: SemanticResource::new("workspace.event", std::iter::empty::<&str>()),
                recursive: true,
            }],
            limitations: vec![
                "Checks event-name routing and consumes only; payload schemas are not checked."
                    .into(),
                "Does not establish the body implementation's file, network, or process effects."
                    .into(),
            ],
        };
        let contracts = [
            ContractClaim {
                role: "mounted_declaration".into(),
                claim: mounted.clone(),
                evidence: attempt
                    .facts
                    .iter()
                    .filter(|f| f.kind == "spool.declaration")
                    .cloned()
                    .collect(),
            },
            ContractClaim {
                role: "workspace_route".into(),
                claim: register.clone(),
                evidence: attempt
                    .facts
                    .iter()
                    .filter(|f| {
                        f.kind == "workspace.registration"
                            || f.kind == "workspace.registration.generation"
                    })
                    .cloned()
                    .collect(),
            },
        ];
        self.authorize_attempt(agent, &attempt, &[mounted, register], &profile, &contracts)
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

    fn body_named(&self, name: &str) -> Option<Mounted> {
        let bodies = self
            .inner
            .bodies
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        bodies.iter().find(|mounted| mounted.name == name).cloned()
    }

    fn drop_body(&self, instance: Uuid) -> Result<(), ControlError> {
        let mut state = self
            .inner
            .supervision
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut bodies = self
            .inner
            .bodies
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        bodies.retain(|mounted| mounted.instance != instance);
        drop(bodies);
        let requests = state.unmount(instance);
        self.record_review_closures(requests, "body_failed")
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
