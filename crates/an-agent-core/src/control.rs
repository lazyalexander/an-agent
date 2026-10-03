//! Host face in this process. The host opens a thread, advances it, cancels
//! the current beat, and finishes it. One thread has one writer and one tape.
//! The tape is born here.
//!
//! Scheduling, mount, mail, and code execution belong on this face. This step
//! has no workspace, no spool, and no session product. The pulse never enters
//! a spool: there is nothing to poll. `spawn_with` stays a card-and-tape
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
use crate::memstream::{AppendEvent, FromKind, Kind, Memevent, StoreError};
use crate::principal::card::AgentCard;
use crate::principal::factory::ToolCtor;

use crate::agent::{Agent, SpawnError, spawn_with};
use crate::seat::{Pool, PoolError, Seat, TreeError};
use crate::workspace::{Workspace, WorkspaceCite, WorkspaceRecord};

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
            }),
        })
    }

    /// Append one software event. Repeated text is a new event.
    pub fn push_event(&self, body: &str) -> Result<WorkspaceRecord, ControlError> {
        Ok(self.inner.workspace.push_event(body)?)
    }

    /// Store precise bytes. Identical bytes keep the previous id.
    pub fn put_config(&self, bytes: &[u8]) -> Result<WorkspaceRecord, ControlError> {
        Ok(self.inner.workspace.put_config(bytes)?)
    }

    /// Replace the environment markdown. Identical text keeps the previous id.
    pub fn put_env(&self, markdown: &str) -> Result<WorkspaceRecord, ControlError> {
        Ok(self.inner.workspace.put_env(markdown)?)
    }

    /// Current config id, env generation id, and env text.
    pub fn workspace_cite(&self) -> Result<WorkspaceCite, ControlError> {
        Ok(self.inner.workspace.cite()?)
    }

    /// The workspace log. This is the listen port. It does not poll.
    pub fn workspace_log(&self) -> Result<Vec<WorkspaceRecord>, ControlError> {
        Ok(self.inner.workspace.log()?)
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
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::act::{Permit, Tool, ToolCtx, ToolTag};
    use crate::principal::card::{ModelSpec, ToolGrant, Topology};
    use crate::testkit::{TempDir, bash_registry};
    use async_trait::async_trait;
    use serde_json::Value;

    fn card(id: &str, name: &str, permit: Permit) -> AgentCard {
        AgentCard {
            v: 1,
            id: Uuid::parse_str(id).unwrap(),
            model: ModelSpec {
                base_url: "https://example.com".into(),
                model: "m".into(),
                extra_body: None,
            },
            prompt: "p".into(),
            tools: vec![ToolGrant {
                name: name.into(),
                tag: ToolTag::none_permit(permit),
            }],
            topology: Topology::Leaf,
            kernel: "0.1.0".into(),
            supersedes: None,
        }
    }

    struct Gate {
        started: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    }

    #[async_trait]
    impl Tool for Gate {
        fn name(&self) -> &str {
            "gate"
        }
        fn description(&self) -> &str {
            "holds the beat until released"
        }
        fn parameters(&self) -> Value {
            json!({})
        }
        async fn execute(&self, _args: Value, ctx: &ToolCtx) -> Result<String, String> {
            self.started.notify_one();
            let abort = async {
                if let Some(mut rx) = ctx.signal.clone() {
                    loop {
                        if *rx.borrow() {
                            return;
                        }
                        if rx.changed().await.is_err() {
                            std::future::pending::<()>().await;
                        }
                    }
                } else {
                    std::future::pending::<()>().await;
                }
            };
            tokio::select! {
                _ = abort => Ok("cancelled".into()),
                _ = self.release.notified() => Ok("ran".into()),
            }
        }
    }

    #[test]
    fn open_thread_births_one_tape() {
        let tmp = TempDir::new("control-open");
        let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
        let id = control
            .open_thread(&card(
                "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
                "bash",
                Permit::Deny,
            ))
            .unwrap();
        let events = control.events(id).unwrap();
        assert_eq!(events.len(), 1);
        assert!(events[0].tags.iter().any(|tag| tag == "thread"));
        assert!(events[0].content.contains(&id.to_string()));
        assert!(matches!(
            control.open_thread(&card(
                "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
                "bash",
                Permit::Deny,
            )),
            Err(ControlError::Tree(TreeError::Duplicate(_)))
        ));
    }

    #[test]
    fn advance_then_finish_refuses_another_beat() {
        let tmp = TempDir::new("control-finish");
        let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
        let id = control
            .open_thread(&card(
                "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
                "bash",
                Permit::Deny,
            ))
            .unwrap();
        control.advance(id, "look here").unwrap();
        control.finish(id, "done").unwrap();
        assert!(matches!(
            control.advance(id, "again"),
            Err(ControlError::Finished(_))
        ));
        let events = control.events(id).unwrap();
        assert!(events.iter().any(|event| {
            event.tags.iter().any(|tag| tag == "advance") && event.content.contains("look here")
        }));
        assert!(
            events
                .iter()
                .any(|event| event.tags.iter().any(|tag| tag == "finish"))
        );
    }

    #[tokio::test]
    async fn cancel_consumes_one_beat_and_does_not_run_the_tool() {
        let tmp = TempDir::new("control-cancel");
        let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
        let id = control
            .open_thread(&card(
                "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
                "bash",
                Permit::Go,
            ))
            .unwrap();
        control.cancel(id).unwrap();
        let call = ToolCall {
            id: "c1".into(),
            name: "bash".into(),
            arguments: r#"{"command":"true"}"#.into(),
        };
        assert!(matches!(
            control.run(id, &call).await,
            Err(ControlError::Cancelled(_))
        ));
        control.advance(id, "next").unwrap();
        let events = control.events(id).unwrap();
        assert!(
            events
                .iter()
                .any(|event| event.tags.iter().any(|tag| tag == "cancel"))
        );
        assert!(events.iter().any(|event| event.content.contains("next")));
        assert!(events.iter().all(|event| event.content != "ran"));
    }

    #[tokio::test]
    async fn a_running_beat_holds_the_only_slot() {
        let tmp = TempDir::new("control-slot");
        let gate = shared_gate();
        let control = AgentControl::open(tmp.path(), &[("gate", gate_ctor)]).unwrap();
        let running = control
            .open_thread(&card(
                "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
                "gate",
                Permit::Go,
            ))
            .unwrap();
        let other = control
            .open_thread(&card(
                "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
                "gate",
                Permit::Go,
            ))
            .unwrap();
        let worker = control.clone();
        let task = tokio::spawn(async move {
            worker
                .run(
                    running,
                    &ToolCall {
                        id: "c1".into(),
                        name: "gate".into(),
                        arguments: "{}".into(),
                    },
                )
                .await
        });
        gate.started.notified().await;
        assert!(matches!(
            control.advance(other, "wait"),
            Err(ControlError::Pool(PoolError::AtCapacity(1)))
        ));
        gate.release.notify_one();
        let result = task.await.unwrap().unwrap();
        assert_eq!(result.message.content, "ran");
        control.advance(other, "wait").unwrap();
    }

    fn shared_gate() -> &'static Gate {
        use std::sync::OnceLock;
        static GATE: OnceLock<Gate> = OnceLock::new();
        GATE.get_or_init(|| Gate {
            started: Arc::new(tokio::sync::Notify::new()),
            release: Arc::new(tokio::sync::Notify::new()),
        })
    }

    fn gate_ctor() -> Arc<dyn Tool> {
        Arc::new(SharedGate(shared_gate()))
    }

    struct SharedGate(&'static Gate);

    #[async_trait]
    impl Tool for SharedGate {
        fn name(&self) -> &str {
            self.0.name()
        }
        fn description(&self) -> &str {
            self.0.description()
        }
        fn parameters(&self) -> Value {
            self.0.parameters()
        }
        async fn execute(&self, args: Value, ctx: &ToolCtx) -> Result<String, String> {
            self.0.execute(args, ctx).await
        }
    }

    #[test]
    fn send_writes_both_tapes() {
        let tmp = TempDir::new("control-send");
        let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
        let from = control
            .open_thread(&card(
                "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
                "bash",
                Permit::Deny,
            ))
            .unwrap();
        let to = control
            .open_thread(&card(
                "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
                "bash",
                Permit::Deny,
            ))
            .unwrap();
        control.send(from, to, "ping").unwrap();
        assert!(matches!(
            control.send(from, from, "no"),
            Err(ControlError::SameThread(_))
        ));
        assert!(control.events(from).unwrap().iter().any(|event| {
            event.tags.iter().any(|tag| tag == "send") && event.content.contains("ping")
        }));
        assert!(control.events(to).unwrap().iter().any(|event| {
            event.tags.iter().any(|tag| tag == "send")
                && event.content.contains("ping")
                && event.content.contains(&from.to_string())
        }));
    }
}
