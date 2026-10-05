//! Cancel, mail, and the turn queue. Rights stay on the tape.

use super::*;

impl Recorder {
    /// Parent cancels a direct child. Recorded on both tapes. A child cannot
    /// cancel its parent or a sibling.
    pub fn cancel(&self, parent: Uuid, child: Uuid) -> Result<String, RecorderError> {
        let actual = self.pool.tree().parent(child)?;
        if actual != Some(parent) {
            return Err(RecorderError::SignalDenied("cancel"));
        }
        let child_agent = self
            .pool
            .tree()
            .get(child)
            .ok_or(RecorderError::NotMounted(child))?;
        let parent_agent = self
            .pool
            .tree()
            .get(parent)
            .ok_or(RecorderError::NotMounted(parent))?;
        self.record_both(&parent_agent, &child_agent, "cancel", "cancel")
    }

    /// Set this seat's cancel flag and record cancel down every child edge.
    /// The flag is the only thing a [`Lease`] carries.
    pub fn propagate_cancel(&self, id: Uuid) -> Result<(), RecorderError> {
        if self
            .flags
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .is_none()
            && id != self.agent.id()
        {
            return Err(RecorderError::NotMounted(id));
        }
        self.mark_cancelled(id);
        let children = self.pool.tree().children(id).unwrap_or_default();
        for child in children {
            self.propagate_cancel(child)?;
        }
        if id != self.agent.id() {
            let parent = self
                .pool
                .tree()
                .parent(id)?
                .ok_or(RecorderError::NotMounted(id))?;
            self.cancel(parent, id)?;
        }
        Ok(())
    }

    pub fn lease(&self, id: Uuid) -> Result<Lease, RecorderError> {
        let flags_map = self.flags.lock().unwrap_or_else(|e| e.into_inner());
        let mut flags = Vec::new();
        let mut cursor = id;
        loop {
            let flag = flags_map
                .get(&cursor)
                .cloned()
                .ok_or(RecorderError::NotMounted(cursor))?;
            flags.push(flag);
            if cursor == self.agent.id() {
                break;
            }
            cursor = self
                .pool
                .tree()
                .parent(cursor)?
                .ok_or(RecorderError::NotMounted(cursor))?;
        }
        Ok(Lease { id, flags })
    }

    /// Child finishes and hands a result to its parent.
    /// Denied when the charter's complete face is Deny.
    pub fn complete(&self, child: Uuid, text: &str) -> Result<String, RecorderError> {
        let parent = self
            .pool
            .tree()
            .parent(child)?
            .ok_or(RecorderError::NotMounted(child))?;
        let bound = self.charter(child)?;
        if bound.signal.complete == Permit::Deny {
            return Err(RecorderError::SignalDenied("complete"));
        }
        let child_agent = self
            .pool
            .tree()
            .get(child)
            .ok_or(RecorderError::NotMounted(child))?;
        let parent_agent = self
            .pool
            .tree()
            .get(parent)
            .ok_or(RecorderError::NotMounted(parent))?;
        self.record_both(&child_agent, &parent_agent, "complete", text)
    }

    /// Child sends a message. The parent is always allowed.
    /// Anyone else requires `audience: any`.
    pub fn send(&self, from: Uuid, to: Uuid, text: &str) -> Result<String, RecorderError> {
        let parent = self
            .pool
            .tree()
            .parent(from)?
            .ok_or(RecorderError::NotMounted(from))?;
        let bound = self.charter(from)?;
        if to != parent && bound.signal.audience != Audience::Any {
            return Err(RecorderError::SignalDenied("send"));
        }
        let from_agent = self
            .pool
            .tree()
            .get(from)
            .ok_or(RecorderError::NotMounted(from))?;
        let to_agent = self
            .pool
            .tree()
            .get(to)
            .ok_or(RecorderError::NotMounted(to))?;
        self.record_both(&from_agent, &to_agent, "send", text)
    }

    pub fn submit(&self, worker: Uuid, text: &str) -> Result<Arrival, RecorderError> {
        if self.pool.is_running(worker) {
            self.mail(worker, text)?;
            return Ok(Arrival::Steered);
        }
        self.queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_back(Intent {
                worker,
                text: text.to_string(),
            });
        Ok(Arrival::Queued)
    }

    pub fn pop(&self) -> Result<Intent, RecorderError> {
        if self.pool.running() > 0 {
            return Err(RecorderError::Busy);
        }
        self.queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pop_front()
            .ok_or(RecorderError::Empty)
    }

    pub fn begin_turn(&self, worker: Uuid) -> Result<Turn<'_>, RecorderError> {
        if self.pool.tree().get(worker).is_none() {
            return Err(RecorderError::NotMounted(worker));
        }
        Ok(self.pool.begin_turn(worker)?)
    }

    pub fn mail(&self, worker: Uuid, text: &str) -> Result<String, RecorderError> {
        let child = self
            .pool
            .tree()
            .get(worker)
            .ok_or(RecorderError::NotMounted(worker))?;
        let body = json!({
            "to_session": child.session().id_str(),
            "text": text,
        })
        .to_string();
        let parent = self.append(&self.agent, "send", body, vec![])?;
        let back = json!({
            "to_session": self.agent.session().id_str(),
            "text": text,
        })
        .to_string();
        self.append(&child, "send", back, vec![parent.clone()])?;
        Ok(parent)
    }

    fn mark_cancelled(&self, id: Uuid) {
        if let Some(flag) = self
            .flags
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
        {
            flag.store(true, Ordering::Relaxed);
        }
    }
}
