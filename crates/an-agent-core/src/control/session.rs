//! Seal, release, and resume. The live workspace is not rolled back.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use tokio::sync::watch;
use uuid::Uuid;

use super::{AgentControl, AgentRunState, ControlError};
use crate::agent::{SessionError, SessionManifest, spawn_resume};
use crate::det_seam::Clock;
use crate::memstream::Kind;
use crate::principal::card::AgentCard;
use crate::seat::TreeError;

impl AgentControl {
    /// Bind this agent's directory and context index to the current
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

    /// Drop a finished agent's seat. The directory stays on disk.
    pub fn release(&self, id: Uuid) -> Result<(), ControlError> {
        let mut state = self
            .inner
            .supervision
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if self.inner.pool.is_running(id) {
            return Err(ControlError::Busy(id));
        }
        {
            let agent_runs = self
                .inner
                .agent_runs
                .lock()
                .unwrap_or_else(|err| err.into_inner());
            let run_state = agent_runs.get(&id).ok_or(ControlError::NotOpen(id))?;
            if !run_state.finished {
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
        let requests = state.retire(id);
        let audit = self.record_review_closures(requests, "release");
        let seat = seats.remove(pos);
        drop(seats);
        seat.unmount();
        self.inner
            .agent_runs
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .remove(&id);
        audit
    }

    /// Mount a sealed session. The card must be the one named by the manifest.
    /// Does not change the live workspace.
    pub fn resume_session(&self, card: &AgentCard, session_id: Uuid) -> Result<Uuid, ControlError> {
        let mut state = self
            .inner
            .supervision
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if self
            .inner
            .agent_runs
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
        let pending_finish = agent
            .session()
            .tape()
            .read_all()?
            .into_iter()
            .rev()
            .find(|e| {
                e.tags
                    .iter()
                    .any(|t| t == "finish" || t == "finish_pending")
            })
            .filter(|e| e.tags.iter().any(|t| t == "finish_pending"))
            .map(|e| {
                serde_json::from_str::<serde_json::Value>(&e.content)
                    .map_err(|e| ControlError::SemanticRecord(e.to_string()))
                    .and_then(|v| {
                        v["text"].as_str().map(str::to_owned).ok_or_else(|| {
                            ControlError::SemanticRecord("invalid pending finish".into())
                        })
                    })
            })
            .transpose()?;
        let id = agent.id();
        let instance = super::InstanceRef {
            principal: id,
            incarnation: crate::det_seam::Entropy::os().uuid_v4(),
        };
        let seat = self.inner.pool.tree().mount(Arc::clone(&agent), None)?;
        state.open(instance)?;
        #[cfg(test)]
        if self
            .inner
            .fail_lifecycle_append
            .swap(false, std::sync::atomic::Ordering::Relaxed)
        {
            agent.session().tape().fail_next_append();
        }
        if let Err(error) = self.append(
            &agent,
            "resume",
            serde_json::json!({"session":session_id.to_string(),"instance":instance}).to_string(),
        ) {
            state.retire(id);
            return Err(error);
        }
        self.install_live(agent, seat);
        if let Some(text) = pending_finish {
            state.retire(id);
            let mut runs = self
                .inner
                .agent_runs
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let run = runs.get_mut(&id).ok_or(ControlError::NotOpen(id))?;
            run.finishing = true;
            run.finish_text = Some(text);
        }
        Ok(id)
    }
    pub(super) fn install_live(&self, agent: Arc<crate::agent::Agent>, seat: crate::seat::Seat) {
        self.inner
            .seats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(seat);
        let (cancel, _) = watch::channel(false);
        self.inner
            .agent_runs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                agent.id(),
                AgentRunState {
                    finished: false,
                    finishing: false,
                    finish_reviews: Vec::new(),
                    finish_text: None,
                    failed_deliveries: Default::default(),
                    cancelled: Arc::new(AtomicBool::new(false)),
                    cancel,
                },
            );
    }

    pub(super) fn finish_session(&self, id: Uuid, text: &str) -> Result<String, ControlError> {
        use std::sync::atomic::Ordering;
        let mut state = self
            .inner
            .supervision
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if self.inner.pool.is_running(id) {
            return Err(ControlError::Busy(id));
        }
        let agent = self.mounted(id)?;
        let mut runs = self
            .inner
            .agent_runs
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let run = runs.get_mut(&id).ok_or(ControlError::NotOpen(id))?;
        if run.finished {
            return Err(ControlError::Finished(id));
        }
        if !run.finishing {
            // Persist intent before retiring authority. An incomplete finish is retryable,
            // but may never admit another beat or restore a consumed approval.
            self.append(
                &agent,
                "finish_pending",
                serde_json::json!({"text":text}).to_string(),
            )?;
            run.finishing = true;
            run.finish_text = Some(text.to_owned());
            run.finish_reviews = state.retire(id);
        }
        let requests = run.finish_reviews.clone();
        let text = run.finish_text.clone().ok_or(ControlError::Finishing(id))?;
        drop(runs);
        self.record_review_closures(requests, "finish")?;
        let note = self.append(
            &agent,
            "finish",
            serde_json::json!({"text":text}).to_string(),
        )?;
        let mut runs = self
            .inner
            .agent_runs
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let run = runs.get_mut(&id).ok_or(ControlError::NotOpen(id))?;
        run.finished = true;
        run.finishing = false;
        run.cancelled.store(false, Ordering::Relaxed);
        let _ = run.cancel.send(false);
        Ok(note)
    }
}
