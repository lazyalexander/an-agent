//! Mounted bodies and delivery. A body receives one workspace event.
//! It does not read the log, write the tape, or store config or env.
//! The host lands a reply in software; [`AgentControl::applied`] records
//! that landing on the thread tape.

use std::sync::Arc;

use serde_json::json;
use uuid::Uuid;

use super::{AgentControl, ControlError};
use crate::agent::Agent;
use crate::workspace::WorkspaceRecord;

/// A body that receives one workspace event from [`AgentControl::offer`]
/// or [`AgentControl::dispatch`]. It does not read the log, write the tape,
/// or call another body.
pub trait SpoolBeat: Send + Sync {
    fn receive(&self, event: &WorkspaceRecord) -> Result<String, String>;
}

/// What one offered body returned. The workspace event was already on the log.
/// `tape_id` is the spool note on the thread tape. The host cites it when
/// it records [`AgentControl::applied`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpoolReply {
    pub spool: String,
    pub event_id: String,
    pub reply: String,
    pub tape_id: String,
}

/// A mounted body's claim about workspace events: which it receives
/// (`consumes`) and which it may emit (`produces`). Matched against the
/// register at mount — the bind point — and only there: the delivery path
/// stays name-only. Matching is mutual, both directions:
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
        if bodies.iter().any(|(mounted, _)| mounted == name) {
            return Err(ControlError::AlreadyMounted(name.to_string()));
        }
        bodies.push((name.to_string(), body));
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
        let Some(pos) = bodies.iter().position(|(mounted, _)| mounted == name) else {
            return Err(ControlError::NotMounted(name.to_string()));
        };
        bodies.remove(pos);
        Ok(())
    }

    /// One beat. The named body receives the latest workspace event.
    pub fn offer(&self, id: Uuid, name: &str) -> Result<SpoolReply, ControlError> {
        let event = self.latest_event()?;
        self.offer_on(id, name, &event.id)
    }

    /// One beat. The named body receives the workspace event `event_id`.
    /// Core writes the reply on the thread tape. The body does not.
    /// A failure unmounts that body only. The event stays on the log.
    /// The event passes the before chain; the reply passes the after chain.
    pub fn offer_on(
        &self,
        id: Uuid,
        name: &str,
        event_id: &str,
    ) -> Result<SpoolReply, ControlError> {
        self.enter_beat(id)?;
        let turn = self.inner.pool.begin_turn(id)?;
        let event = self.event_by_id(event_id)?;
        self.hooks_before(turn.agent(), &event)?;
        match self.invoke(turn.agent(), name, &event)? {
            Ok(reply) => {
                self.hooks_after(turn.agent(), &reply)?;
                Ok(reply)
            }
            Err(reason) => Err(ControlError::SpoolFailed {
                name: name.to_string(),
                reason,
            }),
        }
    }

    /// One beat. The latest event goes to each spool on its route, in order.
    pub fn dispatch(&self, id: Uuid) -> Result<Vec<SpoolReply>, ControlError> {
        let event = self.latest_event()?;
        self.dispatch_on(id, &event.id)
    }

    /// One beat. The named event goes to each spool on its route, in order.
    /// A missing spool is noted and skipped. A failed spool is unmounted and
    /// the rest still run. Replies are returned to the host. They are not
    /// written as new workspace events. The event passes the before chain
    /// once; each reply passes the after chain — a denied reply does not
    /// reach the host, and the denial is taped.
    pub fn dispatch_on(&self, id: Uuid, event_id: &str) -> Result<Vec<SpoolReply>, ControlError> {
        self.enter_beat(id)?;
        let turn = self.inner.pool.begin_turn(id)?;
        let event = self.event_by_id(event_id)?;
        self.hooks_before(turn.agent(), &event)?;
        let event_name = event.name.clone().unwrap_or_default();
        let route = self.spools_for(&event_name)?;
        let mut replies = Vec::new();
        for spool in route {
            match self.invoke(turn.agent(), &spool, &event) {
                Ok(Ok(reply)) => match self.hooks_after(turn.agent(), &reply) {
                    Ok(()) => replies.push(reply),
                    Err(ControlError::HookDenied { .. }) => {}
                    Err(err) => return Err(err),
                },
                Ok(Err(_)) => {}
                Err(ControlError::NotMounted(_)) => {
                    self.note_spool(turn.agent(), &spool, &event.id, Err("missing"))?;
                }
                Err(err) => return Err(err),
            }
        }
        drop(turn);
        Ok(replies)
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

    fn invoke(
        &self,
        agent: &Agent,
        name: &str,
        event: &WorkspaceRecord,
    ) -> Result<Result<SpoolReply, String>, ControlError> {
        let Some(body) = self.body_named(name) else {
            return Err(ControlError::NotMounted(name.to_string()));
        };
        match body.receive(event) {
            Ok(reply) => {
                let tape_id = self.note_spool(agent, name, &event.id, Ok(reply.as_str()))?;
                Ok(Ok(SpoolReply {
                    spool: name.to_string(),
                    event_id: event.id.clone(),
                    reply,
                    tape_id,
                }))
            }
            Err(reason) => {
                self.drop_body(name);
                self.note_spool(agent, name, &event.id, Err(reason.as_str()))?;
                Ok(Err(reason))
            }
        }
    }

    fn body_named(&self, name: &str) -> Option<Arc<dyn SpoolBeat>> {
        let bodies = self
            .inner
            .bodies
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        bodies
            .iter()
            .find(|(mounted, _)| mounted == name)
            .map(|(_, body)| Arc::clone(body))
    }

    fn drop_body(&self, name: &str) {
        let mut bodies = self
            .inner
            .bodies
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        bodies.retain(|(mounted, _)| mounted != name);
    }

    fn note_spool(
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
