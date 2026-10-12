//! Host face in this process. The host opens a thread, advances it, cancels
//! the current beat, and finishes it. One thread has one writer and one tape.
//! The tape is born here.
//!
//! Config and env belong to the workspace. The host injects them. Config is
//! a TOML document. Env is markdown. A spool does not store either one.
//!
//! A plugin registers the names this process watches. Writes outside that
//! list are refused. The list is a snapshot: a new list is a new id, and
//! the old bytes stay.
//!
//! `offer` / `dispatch` wake mounted bodies from a workspace event (latest,
//! or a named id via `offer_on` / `dispatch_on`). The body does not read the
//! log, write the tape, or mutate the host. A missing body is noted and
//! skipped. A failed body is unmounted. Replies go to the tape and back to
//! the caller. The host lands them in software, then `applied` records that
//! landing. The workspace event stays. `spawn_with` stays a card-and-tape
//! constructor. It is not this entry.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::json;
use thiserror::Error;
use tokio::sync::watch;
use uuid::Uuid;

use crate::act::{ToolCall, ToolCtx, ToolError, run_tool_act};
use crate::agent::{Agent, SessionError, SpawnError, spawn_with};
use crate::memstream::{AppendEvent, FromKind, Kind, Memevent, StoreError};
use crate::principal::card::AgentCard;
use crate::principal::factory::ToolCtor;
use crate::seat::{Pool, PoolError, Seat, TreeError};
use crate::workspace::Workspace;

mod agent_beat;
mod hooks;
mod session;
mod spool;
mod workspace;

pub use agent_beat::{AgentBody, BeatStep, Completion, ModelClient, ModelMessage, Usage};
pub use hooks::{HookRunner, HookVerdict};
use spool::Mounted;
pub use spool::{DeliveryOutcome, ReplyIntent, SpoolBeat, SpoolDeclaration, SpoolReply};
pub use workspace::{
    EventRoute, Guard, Guidance, HookHandler, Hooks, Registration, SpoolRequirement,
    WorkspaceConfig,
};

#[derive(Debug, Error)]
pub enum ControlError {
    #[error("thread is not open: {0}")]
    NotOpen(Uuid),
    #[error("thread is finished: {0}")]
    Finished(Uuid),
    #[error("thread beat was cancelled: {0}")]
    Cancelled(Uuid),
    #[error("thread is in a beat: {0}")]
    Busy(Uuid),
    #[error("a thread cannot send to itself: {0}")]
    SameThread(Uuid),
    #[error("thread is not finished: {0}")]
    NotFinished(Uuid),
    #[error("session manifest does not match this card")]
    ManifestMismatch,
    #[error("spool is not mounted: {0}")]
    NotMounted(String),
    #[error("spool is already mounted: {0}")]
    AlreadyMounted(String),
    #[error("spool {name} does not match the register: {}", .reasons.join("; "))]
    MountMismatch { name: String, reasons: Vec<String> },
    #[error("workspace has no event")]
    NoEvent,
    #[error("workspace has no such event: {0}")]
    UnknownEvent(String),
    #[error("hook denied on the {chain} chain: {reason}")]
    HookDenied { chain: String, reason: String },
    #[error("config declares hooks but no hook runner is installed")]
    HookRunnerMissing,
    #[error("agent body asked for a model call but no model client is installed")]
    ModelClientMissing,
    #[error("model call for spool {spool} failed: {reason}")]
    ModelFailed { spool: String, reason: String },
    #[error("tape has no such clip: {0}")]
    UnknownClip(String),
    #[error("spool {name} failed: {reason}")]
    SpoolFailed { name: String, reason: String },
    #[error("workspace has no registration")]
    NotRegistered,
    #[error("name is not registered: {0}")]
    Unregistered(String),
    #[error("registration is invalid: {0}")]
    InvalidRegistration(String),
    #[error("config is not valid toml: {0}")]
    InvalidConfig(String),
    #[error(transparent)]
    Spawn(#[from] SpawnError),
    #[error(transparent)]
    Pool(#[from] PoolError),
    #[error(transparent)]
    Tree(#[from] TreeError),
    #[error(transparent)]
    Tool(#[from] ToolError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Workspace(#[from] crate::workspace::WorkspaceError),
    #[error(transparent)]
    Session(#[from] SessionError),
}

struct ThreadInner {
    finished: bool,
    /// Raised by `cancel`. The next beat consumes it and does no work.
    cancelled: Arc<AtomicBool>,
    cancel: watch::Sender<bool>,
}

struct Inner {
    root: PathBuf,
    registry: Vec<(String, ToolCtor)>,
    /// One beat at a time. This is not the context distribution pool.
    pool: Pool,
    seats: Mutex<Vec<Seat>>,
    threads: Mutex<HashMap<Uuid, ThreadInner>>,
    workspace: Workspace,
    bodies: Mutex<Vec<Mounted>>,
    hook_runner: Mutex<Option<Arc<dyn HookRunner>>>,
    model_client: Mutex<Option<Arc<dyn ModelClient>>>,
    /// The embedding software's label, stated in the model envelope.
    host_label: Mutex<String>,
}

/// The handle a host software uses to drive agents in this process.
#[derive(Clone)]
pub struct AgentControl {
    inner: Arc<Inner>,
}

impl AgentControl {
    /// `registry` is the tool constructors for every thread opened here.
    /// The kernel registers none. The turn limit is one.
    pub fn open(
        sessions_root: impl AsRef<Path>,
        registry: &[(&str, ToolCtor)],
    ) -> Result<Self, ControlError> {
        Ok(Self {
            inner: Arc::new(Inner {
                root: sessions_root.as_ref().to_path_buf(),
                registry: registry
                    .iter()
                    .map(|(name, ctor)| ((*name).to_string(), *ctor))
                    .collect(),
                pool: Pool::new(1)?,
                seats: Mutex::new(Vec::new()),
                threads: Mutex::new(HashMap::new()),
                workspace: Workspace::open(sessions_root.as_ref().join("workspace"))?,
                bodies: Mutex::new(Vec::new()),
                hook_runner: Mutex::new(None),
                model_client: Mutex::new(None),
                host_label: Mutex::new("an-agent".to_string()),
            }),
        })
    }

    /// Birth one thread and its tape. The id is the card id. Opening the
    /// same card twice is refused.
    pub fn open_thread(&self, card: &AgentCard) -> Result<Uuid, ControlError> {
        if self
            .inner
            .threads
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(&card.id)
        {
            return Err(ControlError::Tree(TreeError::Duplicate(card.id)));
        }
        let listed = self.listed_registry();
        let agent = Arc::new(spawn_with(card, &self.inner.root, &listed)?);
        let id = agent.id();
        let seat = self.inner.pool.tree().mount(Arc::clone(&agent), None)?;
        self.inner
            .seats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(seat);
        let (cancel, _rx) = watch::channel(false);
        self.inner
            .threads
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                id,
                ThreadInner {
                    finished: false,
                    cancelled: Arc::new(AtomicBool::new(false)),
                    cancel,
                },
            );
        self.append(
            &agent,
            "thread",
            json!({ "thread": id.to_string() }).to_string(),
        )?;
        Ok(id)
    }

    /// One beat: record the host's text on the thread tape. A raised cancel
    /// flag consumes the beat and does no work. The host calls again for the
    /// next beat.
    pub fn advance(&self, id: Uuid, text: &str) -> Result<String, ControlError> {
        self.enter_beat(id)?;
        let turn = self.inner.pool.begin_turn(id)?;
        let event = self.append(turn.agent(), "advance", json!({ "text": text }).to_string())?;
        drop(turn);
        Ok(event)
    }

    /// Record cancel and raise the flag. An in-flight [`Self::run`] sees the
    /// flag on its tool context. The next beat is consumed as cancelled.
    pub fn cancel(&self, id: Uuid) -> Result<String, ControlError> {
        let agent = self.live(id)?;
        self.raise_cancel(id)?;
        self.append(
            &agent,
            "cancel",
            json!({ "thread": id.to_string() }).to_string(),
        )
    }

    /// End the thread. Further beats and mail are refused. Refused while a
    /// beat holds the only slot.
    pub fn finish(&self, id: Uuid, text: &str) -> Result<String, ControlError> {
        if self.inner.pool.is_running(id) {
            return Err(ControlError::Busy(id));
        }
        let agent = self.live(id)?;
        let event = self.append(&agent, "finish", json!({ "text": text }).to_string())?;
        let mut threads = self.inner.threads.lock().unwrap_or_else(|e| e.into_inner());
        let thread = threads.get_mut(&id).ok_or(ControlError::NotOpen(id))?;
        thread.finished = true;
        thread.cancelled.store(false, Ordering::Relaxed);
        let _ = thread.cancel.send(false);
        Ok(event)
    }

    /// Mail between two open threads. Each tape gets one event. Does not take
    /// the beat slot.
    pub fn send(&self, from: Uuid, to: Uuid, text: &str) -> Result<String, ControlError> {
        if from == to {
            return Err(ControlError::SameThread(from));
        }
        let origin = self.live(from)?;
        let dest = self.live(to)?;
        let outbound = self.append(
            &origin,
            "send",
            json!({ "to": to.to_string(), "text": text }).to_string(),
        )?;
        self.append(
            &dest,
            "send",
            json!({ "from": from.to_string(), "text": text }).to_string(),
        )?;
        Ok(outbound)
    }

    /// Run one granted tool on the thread tape. Code execution stays on this
    /// face. There is no sandbox here.
    pub async fn run(
        &self,
        id: Uuid,
        call: &ToolCall,
    ) -> Result<crate::act::ToolActResult, ControlError> {
        self.enter_beat(id)?;
        let agent = self
            .inner
            .pool
            .tree()
            .get(id)
            .ok_or(ControlError::NotOpen(id))?;
        let turn = self.inner.pool.begin_turn(id)?;
        let signal = self.cancel_receiver(id)?;
        let result = run_tool_act(
            &agent.act_ctx(),
            agent.tools(),
            call,
            &ToolCtx {
                signal: Some(signal),
            },
        )
        .await?;
        drop(turn);
        Ok(result)
    }

    /// Read the thread tape. Not a writer.
    pub fn events(&self, id: Uuid) -> Result<Vec<Memevent>, ControlError> {
        let agent = self
            .inner
            .pool
            .tree()
            .get(id)
            .ok_or(ControlError::NotOpen(id))?;
        Ok(agent.session().tape().read_all()?)
    }

    fn listed_registry(&self) -> Vec<(&str, ToolCtor)> {
        self.inner
            .registry
            .iter()
            .map(|(name, ctor)| (name.as_str(), *ctor))
            .collect()
    }

    fn live(&self, id: Uuid) -> Result<Arc<Agent>, ControlError> {
        let threads = self.inner.threads.lock().unwrap_or_else(|e| e.into_inner());
        let thread = threads.get(&id).ok_or(ControlError::NotOpen(id))?;
        if thread.finished {
            return Err(ControlError::Finished(id));
        }
        drop(threads);
        self.inner
            .pool
            .tree()
            .get(id)
            .ok_or(ControlError::NotOpen(id))
    }

    /// A beat that finds the cancel flag raised does nothing and lowers it.
    fn enter_beat(&self, id: Uuid) -> Result<(), ControlError> {
        let threads = self.inner.threads.lock().unwrap_or_else(|e| e.into_inner());
        let thread = threads.get(&id).ok_or(ControlError::NotOpen(id))?;
        if thread.finished {
            return Err(ControlError::Finished(id));
        }
        if thread.cancelled.swap(false, Ordering::Relaxed) {
            let _ = thread.cancel.send(false);
            return Err(ControlError::Cancelled(id));
        }
        Ok(())
    }

    fn raise_cancel(&self, id: Uuid) -> Result<(), ControlError> {
        let threads = self.inner.threads.lock().unwrap_or_else(|e| e.into_inner());
        let thread = threads.get(&id).ok_or(ControlError::NotOpen(id))?;
        if thread.finished {
            return Err(ControlError::Finished(id));
        }
        thread.cancelled.store(true, Ordering::Relaxed);
        let _ = thread.cancel.send(true);
        Ok(())
    }

    fn cancel_receiver(&self, id: Uuid) -> Result<watch::Receiver<bool>, ControlError> {
        let threads = self.inner.threads.lock().unwrap_or_else(|e| e.into_inner());
        let thread = threads.get(&id).ok_or(ControlError::NotOpen(id))?;
        Ok(thread.cancel.subscribe())
    }

    /// Private directory of the thread. Context files live under it.
    pub fn directory(&self, id: Uuid) -> Result<PathBuf, ControlError> {
        Ok(self.mounted(id)?.session().root().to_path_buf())
    }

    /// Card prompt for this thread.
    pub fn prompt(&self, id: Uuid) -> Result<String, ControlError> {
        Ok(self.mounted(id)?.prompt().to_string())
    }

    /// Append a host utterance. Does not take the beat slot.
    pub fn note(&self, id: Uuid, tag: &str, text: &str) -> Result<String, ControlError> {
        let agent = self.live(id)?;
        self.append_event(&agent, Kind::Utterance, tag, text.to_string(), Vec::new())
    }

    /// Record a cut the assembler already wrote under `ctx/`.
    pub fn commit_compress(
        &self,
        id: Uuid,
        md_sha256: &str,
        event_ids: &[String],
    ) -> Result<String, ControlError> {
        let agent = self.live(id)?;
        let content = json!({ "md_sha256": md_sha256, "events": event_ids }).to_string();
        self.append_event(
            &agent,
            Kind::Action,
            "compress",
            content,
            event_ids.to_vec(),
        )
    }

    /// Record the piece ids chosen for the next read. The text stays in `ctx/`.
    pub fn commit_context(
        &self,
        id: Uuid,
        content: &str,
        refs: &[String],
    ) -> Result<String, ControlError> {
        let agent = self.live(id)?;
        self.append_event(
            &agent,
            Kind::Action,
            "context",
            content.to_string(),
            refs.to_vec(),
        )
    }
    fn mounted(&self, id: Uuid) -> Result<Arc<Agent>, ControlError> {
        self.inner
            .pool
            .tree()
            .get(id)
            .ok_or(ControlError::NotOpen(id))
    }

    fn append(&self, agent: &Agent, tag: &str, content: String) -> Result<String, ControlError> {
        self.append_event(agent, Kind::Action, tag, content, Vec::new())
    }

    fn append_event(
        &self,
        agent: &Agent,
        kind: Kind,
        tag: &str,
        content: String,
        refs: Vec<String>,
    ) -> Result<String, ControlError> {
        let event = agent.session().tape().append(AppendEvent {
            from: agent.id_str().to_string(),
            from_kind: FromKind::Agent,
            kind,
            session: agent.session().id_str().to_string(),
            content,
            tags: vec![tag.to_string()],
            refs,
            act: None,
            card: Some(agent.card_hash().to_string()),
        })?;
        Ok(event.id)
    }
}

#[cfg(test)]
mod tests;
