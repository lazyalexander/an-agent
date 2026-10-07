//! The embedding surface. Host software (a chat bridge, an editor, a
//! drawing app) links this crate and gets the two glue loops every
//! embedding needs, so each host stops hand-rolling them:
//!
//! - [`Host::mount_from_config`] — read the workspace's `[[spool]]`
//!   requirements, recover each pinned body from the registry, construct
//!   it (rhai from the shelf; `constructor: host` from the app's own
//!   [`HostBodies`]), and mount it under its instance name with its mount
//!   config. One body, many instances: personas are config, not code.
//! - [`Host::pump`] / [`Host::pump_on`] — one beat, host side: dispatch
//!   an event, execute each reply's intents (utterances go to the app's
//!   sink; raises become new workspace events), then record [`applied`]
//!   so the tape closes the loop event → spool note → applied.
//!
//! Deliberately not here: raised events are not re-dispatched inside a
//! pump — the loop bound and the self-loop guard stay the app's call
//! (the host has initiative; core never wakes anyone on its own). The
//! hook runner is also absent: it waits on the MCP shape, and lands as
//! its own slice.
//!
//! [`applied`]: an_agent_core::control::AgentControl::applied

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Map, Value};
use thiserror::Error;
use uuid::Uuid;

use an_agent_core::control::{
    AgentControl, ControlError, ReplyIntent, SpoolBeat, SpoolReply, SpoolRequirement,
};
use an_agent_spool::beat::RhaiBeat;
use an_agent_spool::spool::{Constructor, Registry, SpoolError};

#[derive(Debug, Error)]
pub enum HostError {
    #[error(transparent)]
    Control(#[from] ControlError),
    #[error(transparent)]
    Spool(#[from] SpoolError),
    #[error("mount config is not representable: {0}")]
    Config(#[from] serde_json::Error),
    #[error("constructor: host names a body the app did not register: {0}")]
    UnknownHostBody(String),
    #[error("body construction failed: {0}")]
    Body(String),
}

type HostCtor = Arc<dyn Fn(Map<String, Value>) -> Result<Arc<dyn SpoolBeat>, String> + Send + Sync>;

/// Native bodies the embedding app ships itself, by the name a
/// `constructor: host` spool body gives. The discord bridge, the editor
/// channel, the canvas sink — code only the app can write.
#[derive(Default)]
pub struct HostBodies {
    ctors: HashMap<String, HostCtor>,
}

impl HostBodies {
    pub fn register<F, B>(&mut self, name: &str, ctor: F) -> &mut Self
    where
        F: Fn(Map<String, Value>) -> Result<B, String> + Send + Sync + 'static,
        B: SpoolBeat + 'static,
    {
        self.ctors.insert(
            name.to_string(),
            Arc::new(move |config| {
                let body = ctor(config)?;
                Ok(Arc::new(body) as Arc<dyn SpoolBeat>)
            }),
        );
        self
    }

    fn construct(
        &self,
        name: &str,
        config: Map<String, Value>,
    ) -> Result<Arc<dyn SpoolBeat>, HostError> {
        let ctor = self
            .ctors
            .get(name)
            .ok_or_else(|| HostError::UnknownHostBody(name.to_string()))?;
        ctor(config).map_err(HostError::Body)
    }
}

/// The host handle: an [`AgentControl`] plus the spool registry plus the
/// app's native bodies. Construction of the control (its tool registry)
/// and the registry stay the app's business; the host only composes them.
pub struct Host {
    control: AgentControl,
    registry: Registry,
    host_bodies: HostBodies,
}

impl Host {
    pub fn new(control: AgentControl, registry: Registry, host_bodies: HostBodies) -> Self {
        Self {
            control,
            registry,
            host_bodies,
        }
    }

    /// Escape hatch: event pushing, registration, tape reads.
    pub fn control(&self) -> &AgentControl {
        &self.control
    }

    /// Mount every `[[spool]]` in the current workspace config. Returns
    /// the instance names mounted, in config order. A requirement whose
    /// pinned body is missing from the registry fails the whole pass —
    /// a half-mounted workspace is worse than a refused one.
    pub fn mount_from_config(&self) -> Result<Vec<String>, HostError> {
        let Some(config) = self.control.workspace_config()? else {
            return Ok(Vec::new());
        };
        let mut mounted = Vec::new();
        for req in &config.spool {
            let (instance, body, declaration) = self.construct(req)?;
            self.control.mount_spool(&instance, body, &declaration)?;
            mounted.push(instance);
        }
        Ok(mounted)
    }

    fn construct(
        &self,
        req: &SpoolRequirement,
    ) -> Result<
        (
            String,
            Arc<dyn SpoolBeat>,
            an_agent_core::control::SpoolDeclaration,
        ),
        HostError,
    > {
        let spec = self.registry.recover(&req.name, &req.version)?;
        let config = match serde_json::to_value(&req.config)? {
            Value::Object(map) => map,
            other => {
                return Err(HostError::Body(format!(
                    "mount config is {other}, not a table"
                )));
            }
        };
        let instance = req.mount.clone().unwrap_or_else(|| req.name.clone());
        let body: Arc<dyn SpoolBeat> = match &spec.constructor {
            Constructor::Rhai { .. } => {
                Arc::new(RhaiBeat::from_spool(&spec, config).map_err(HostError::Body)?)
            }
            Constructor::Host { name } => self.host_bodies.construct(name, config)?,
        };
        Ok((instance, body, spec.declaration()))
    }

    /// One beat: dispatch the latest workspace event, execute the
    /// replies' intents, record the landings. Returns the replies.
    pub fn pump(&self, id: Uuid, sink: &mut dyn FnMut(&str)) -> Result<Vec<SpoolReply>, HostError> {
        let replies = self.control.dispatch(id)?;
        self.execute(id, replies, sink)
    }

    /// One beat on a named event — backlog processing, in the caller's
    /// order rather than the log's.
    pub fn pump_on(
        &self,
        id: Uuid,
        event_id: &str,
        sink: &mut dyn FnMut(&str),
    ) -> Result<Vec<SpoolReply>, HostError> {
        let replies = self.control.dispatch_on(id, event_id)?;
        self.execute(id, replies, sink)
    }

    /// Execute each reply's intents, then tape the landing. Utterances go
    /// to the app's sink; raises become new workspace events — admission
    /// against `produces` already happened in core, so a raise here is
    /// always one the spool declared. Raised events are left pending:
    /// whether to pump again, and how deep, is the app's call.
    fn execute(
        &self,
        id: Uuid,
        replies: Vec<SpoolReply>,
        sink: &mut dyn FnMut(&str),
    ) -> Result<Vec<SpoolReply>, HostError> {
        for reply in &replies {
            for intent in &reply.intents {
                match intent {
                    ReplyIntent::Utter { text } => sink(text),
                    ReplyIntent::Raise { event, body } => {
                        self.control.push_event(event, body)?;
                    }
                }
            }
            self.control.applied(id, reply, "host pump")?;
        }
        Ok(replies)
    }
}
