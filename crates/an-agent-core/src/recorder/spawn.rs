//! Spawn a worker, a nested worker, or a lite seat, and release a subtree.

use super::*;

impl Recorder {
    /// `bound` is the parent's charter for this child. It is written on the
    /// recorder tape. Every tool grant on `card` must fit inside it.
    /// Registers one sub-workspace for the child.
    pub fn spawn_worker(
        &self,
        card: &AgentCard,
        bound: &Charter,
    ) -> Result<Arc<Agent>, RecorderError> {
        self.spawn_child(self.agent.id(), card, bound, true)
    }

    /// Spawn under an existing child. The new bound must fit in that child's charter.
    pub fn spawn_under(
        &self,
        parent: Uuid,
        card: &AgentCard,
        bound: &Charter,
    ) -> Result<Arc<Agent>, RecorderError> {
        self.spawn_child(parent, card, bound, true)
    }

    /// A child with a seat and no sub-workspace. It cannot write paths.
    /// The charter's file face must be `none`. Finishing is `complete` only.
    pub fn spawn_lite(
        &self,
        card: &AgentCard,
        bound: &Charter,
    ) -> Result<Arc<Agent>, RecorderError> {
        if !bound.file_is_none() {
            return Err(RecorderError::LiteFile);
        }
        self.spawn_child(self.agent.id(), card, bound, false)
    }

    /// Drop this seat and every descendant. Live sub-workspaces are abandoned
    /// with no view. Already published views stay.
    pub fn release(&self, id: Uuid) -> Result<(), RecorderError> {
        let ids = self.pool.tree().descendants(id)?;
        self.append(
            &self.agent,
            "release",
            json!({ "id": id.to_string() }).to_string(),
            vec![],
        )?;
        for worker in &ids {
            self.wp.abandon_worker(*worker)?;
            self.bounds
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(worker);
            self.flags
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(worker);
        }
        let mut seats = self.seats.lock().unwrap_or_else(|e| e.into_inner());
        let mut keep = Vec::new();
        let mut dropping = Vec::new();
        for seat in seats.drain(..) {
            if ids.contains(&seat.id()) {
                dropping.push(seat);
            } else {
                keep.push(seat);
            }
        }
        *seats = keep;
        drop(seats);
        for seat in dropping {
            let _ = seat.unmount();
        }
        Ok(())
    }

    fn spawn_child(
        &self,
        parent: Uuid,
        card: &AgentCard,
        bound: &Charter,
        with_subwp: bool,
    ) -> Result<Arc<Agent>, RecorderError> {
        if self.pool.tree().get(parent).is_none() {
            return Err(RecorderError::NotMounted(parent));
        }
        if parent != self.agent.id() {
            let charter = self.charter(parent)?;
            if !bound.within(&charter) {
                return Err(RecorderError::WiderThanParent);
            }
        }
        for grant in &card.tools {
            if !bound.allows(&grant.tag) {
                return Err(RecorderError::OutsideBound(grant.name.clone()));
            }
        }
        let registry = self.listed_registry();
        let child = Arc::new(spawn_with(card, &self.root, &registry)?);
        let seat = self.pool.tree().mount(Arc::clone(&child), Some(parent))?;
        self.seats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(seat);
        let sub = if with_subwp {
            Some(self.wp.register(child.id(), child.session().root())?)
        } else {
            None
        };
        self.bounds
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(child.id(), bound.clone());
        self.flags
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(child.id(), Arc::new(AtomicBool::new(false)));
        self.append(
            &self.agent,
            "spawn",
            json!({
                "worker": child.id_str(),
                "session": child.session().id_str(),
                "parent": parent.to_string(),
                "subwp": sub,
                "seat": if with_subwp { "worker" } else { "lite" },
                "bound": bound,
            })
            .to_string(),
            vec![],
        )?;
        Ok(child)
    }
}
