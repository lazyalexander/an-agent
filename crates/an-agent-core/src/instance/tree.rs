//! Live registration tree. Parent edges are future Delegate; dropping a
//! seat unregisters the subtree so a child cannot outlive its parent.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use thiserror::Error;
use uuid::Uuid;

use super::agent::Agent;

#[derive(Debug, Error)]
pub enum TreeError {
    #[error("agent already mounted: {0}")]
    Duplicate(Uuid),
    #[error("unknown parent: {0}")]
    UnknownParent(Uuid),
    #[error("unknown agent: {0}")]
    NotFound(Uuid),
}

struct Node {
    agent: Arc<Agent>,
    parent: Option<Uuid>,
    children: BTreeSet<Uuid>,
}

#[derive(Default)]
struct Inner {
    nodes: BTreeMap<Uuid, Node>,
}

#[derive(Clone, Default)]
pub struct Tree {
    inner: Arc<Mutex<Inner>>,
}

impl Tree {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mount(&self, agent: Arc<Agent>, parent: Option<Uuid>) -> Result<Seat, TreeError> {
        let id = agent.id();
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.nodes.contains_key(&id) {
            return Err(TreeError::Duplicate(id));
        }
        if let Some(p) = parent {
            let parent_node = inner.nodes.get_mut(&p).ok_or(TreeError::UnknownParent(p))?;
            parent_node.children.insert(id);
        }
        inner.nodes.insert(
            id,
            Node {
                agent,
                parent,
                children: BTreeSet::new(),
            },
        );
        drop(inner);
        Ok(Seat {
            inner: Arc::clone(&self.inner),
            id,
            armed: true,
        })
    }

    pub fn get(&self, id: Uuid) -> Option<Arc<Agent>> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .nodes
            .get(&id)
            .map(|n| n.agent.clone())
    }

    pub fn parent(&self, id: Uuid) -> Result<Option<Uuid>, TreeError> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .nodes
            .get(&id)
            .map(|n| n.parent)
            .ok_or(TreeError::NotFound(id))
    }

    /// `id` plus every descendant, parent before children.
    pub fn descendants(&self, id: Uuid) -> Result<Vec<Uuid>, TreeError> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if !inner.nodes.contains_key(&id) {
            return Err(TreeError::NotFound(id));
        }
        let mut out = vec![id];
        let mut i = 0;
        while i < out.len() {
            if let Some(node) = inner.nodes.get(&out[i]) {
                out.extend(node.children.iter().copied());
            }
            i += 1;
        }
        Ok(out)
    }

    pub fn children(&self, id: Uuid) -> Result<Vec<Uuid>, TreeError> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .nodes
            .get(&id)
            .map(|n| n.children.iter().copied().collect())
            .ok_or(TreeError::NotFound(id))
    }

    pub fn len(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .nodes
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Owns one mount. Drop unregisters the node and its descendants.
pub struct Seat {
    inner: Arc<Mutex<Inner>>,
    id: Uuid,
    armed: bool,
}

impl Seat {
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn unmount(mut self) -> bool {
        self.armed = false;
        self.remove()
    }

    fn remove(&self) -> bool {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let Some(node) = inner.nodes.remove(&self.id) else {
            return false;
        };
        if let Some(p) = node.parent
            && let Some(parent) = inner.nodes.get_mut(&p)
        {
            parent.children.remove(&self.id);
        }
        let mut stack: Vec<Uuid> = node.children.into_iter().collect();
        while let Some(cid) = stack.pop() {
            if let Some(child) = inner.nodes.remove(&cid) {
                stack.extend(child.children);
            }
        }
        true
    }
}

impl Drop for Seat {
    fn drop(&mut self) {
        if self.armed {
            self.remove();
        }
    }
}
