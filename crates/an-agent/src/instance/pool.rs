//! Bounded turn slots. The loop, not a guest script, occupies a slot.

use std::collections::BTreeSet;
use std::sync::Mutex;

use thiserror::Error;
use uuid::Uuid;

use super::agent::Agent;
use super::tree::{Tree, TreeError};

#[derive(Debug, Error)]
pub enum PoolError {
    #[error("pool limit must be at least 1")]
    ZeroLimit,
    #[error("pool at capacity ({0})")]
    AtCapacity(usize),
    #[error("agent already in a turn: {0}")]
    Busy(Uuid),
    #[error("agent is not mounted: {0}")]
    NotMounted(Uuid),
    #[error(transparent)]
    Tree(#[from] TreeError),
}

pub struct Pool {
    tree: Tree,
    limit: usize,
    running: Mutex<BTreeSet<Uuid>>,
}

/// Occupies one pool slot until dropped.
pub struct Turn<'a> {
    pool: &'a Pool,
    id: Uuid,
    agent: std::sync::Arc<Agent>,
}

impl Pool {
    pub fn new(limit: usize) -> Result<Self, PoolError> {
        if limit == 0 {
            return Err(PoolError::ZeroLimit);
        }
        Ok(Self {
            tree: Tree::new(),
            limit,
            running: Mutex::new(BTreeSet::new()),
        })
    }

    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    pub fn running(&self) -> usize {
        self.running.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub fn begin_turn(&self, id: Uuid) -> Result<Turn<'_>, PoolError> {
        let agent = self.tree.get(id).ok_or(PoolError::NotMounted(id))?;
        let mut running = self.running.lock().unwrap_or_else(|e| e.into_inner());
        if running.contains(&id) {
            return Err(PoolError::Busy(id));
        }
        if running.len() >= self.limit {
            return Err(PoolError::AtCapacity(self.limit));
        }
        running.insert(id);
        drop(running);
        Ok(Turn {
            pool: self,
            id,
            agent,
        })
    }
}

impl Turn<'_> {
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn agent(&self) -> &Agent {
        &self.agent
    }
}

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        self.pool
            .running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id);
    }
}
