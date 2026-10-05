//! Seal, release, and resume. The live workspace is not rolled back.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use tokio::sync::watch;
use uuid::Uuid;

use super::{AgentControl, ControlError, ThreadInner};
use crate::agent::{SessionError, SessionManifest, spawn_resume};
use crate::det_seam::Clock;
use crate::memstream::Kind;
use crate::principal::card::AgentCard;
use crate::seat::TreeError;

impl AgentControl {
    /// Bind this thread's directory and context index to the current
    /// workspace generation. Rewrites `session.json`. Appends one tape event.
    pub fn seal_session(&self, id: Uuid) -> Result<SessionManifest, ControlError> {
        let agent = self.mounted(id)?;
        let cite = self.workspace_cite()?;
        let manifest = SessionManifest {
            id: agent.session().id(),
            agent_id: agent.id(),
            card_hash: agent.card_hash().to_string(),
            config_id: cite.config_id,
            config_sha256: cite.config_sha256,
            env_id: cite.env_id,
            env_sha256: cite.env_sha256,
            register_id: cite.register_id,
            register_sha256: cite.register_sha256,
            at: Clock::wall().now_iso(),
        };
        manifest.write(agent.session().root())?;
        let content = serde_json::to_string(&manifest)
            .map_err(|err| ControlError::Session(SessionError::Manifest(err.to_string())))?;
        self.append_event(&agent, Kind::Action, "session", content, Vec::new())?;
        Ok(manifest)
    }

    /// Drop a finished thread's seat. The directory stays on disk.
    pub fn release(&self, id: Uuid) -> Result<(), ControlError> {
        if self.inner.pool.is_running(id) {
            return Err(ControlError::Busy(id));
        }
        {
            let threads = self
                .inner
                .threads
                .lock()
                .unwrap_or_else(|err| err.into_inner());
            let thread = threads.get(&id).ok_or(ControlError::NotOpen(id))?;
            if !thread.finished {
                return Err(ControlError::NotFinished(id));
            }
        }
        let mut seats = self
            .inner
            .seats
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let Some(pos) = seats.iter().position(|seat| seat.id() == id) else {
            return Err(ControlError::NotOpen(id));
        };
        let seat = seats.remove(pos);
        drop(seats);
        seat.unmount();
        self.inner
            .threads
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .remove(&id);
        Ok(())
    }

    /// Mount a sealed session. The card must be the one named by the manifest.
    /// Does not change the live workspace.
    pub fn resume_session(&self, card: &AgentCard, session_id: Uuid) -> Result<Uuid, ControlError> {
        if self
            .inner
            .threads
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .contains_key(&card.id)
        {
            return Err(ControlError::Tree(TreeError::Duplicate(card.id)));
        }
        let listed = self.listed_registry();
        let agent = Arc::new(spawn_resume(card, &self.inner.root, session_id, &listed)?);
        let manifest = SessionManifest::read(agent.session().root())?;
        if manifest.id != session_id
            || manifest.agent_id != card.id
            || manifest.card_hash != agent.card_hash()
        {
            return Err(ControlError::ManifestMismatch);
        }
        let id = agent.id();
        let seat = self.inner.pool.tree().mount(Arc::clone(&agent), None)?;
        self.inner
            .seats
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(seat);
        let (cancel, _rx) = watch::channel(false);
        self.inner
            .threads
            .lock()
            .unwrap_or_else(|err| err.into_inner())
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
            "resume",
            serde_json::json!({ "session": session_id.to_string() }).to_string(),
        )?;
        Ok(id)
    }
}
