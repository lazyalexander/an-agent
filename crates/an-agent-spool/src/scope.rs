//! Mount scopes: a tree of mounted spool instances where every effect is
//! registered together with its inverse, so unmounting restores whatever
//! the mount found.
//!
//! Semantics, following koishi/cordis:
//!
//! - Instances of one name are **additive siblings**, not a shadow stack:
//!   two mounts of the same body are two independent forks, and either may
//!   leave first. `resolve(name)` answers with the newest active mount;
//!   same-name replacement goes through `update`, never through silent
//!   shadowing.
//! - **Wrapping is the modification channel** ("spool A modifies spool
//!   B"), and it is stack-ordered: a scope cannot unmount while a live
//!   wrapper pins it as a target, and a wrapper can only leave from the
//!   top of its chain. Both violations are hard errors naming the blocker.
//! - Reversibility is captured at the moment of change, not at the moment
//!   of withdrawal: `mount`, `wrap`, and `collect` each register the
//!   inverse with the effect. Children leave before their parent.
//!
//! The tree is pure mechanics: it does not construct instances (the
//! caller does, per constructor kind) and does not tape (the caller turns
//! the returned disposal report into Unmount acts).

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use an_agent_core::act::Tool;
use serde_json::{Map, Value};
use thiserror::Error;

use crate::spool::Inverse;

/// Handle to a mounted scope. Operations on a disposed scope are hard
/// errors, never silent no-ops.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScopeId(u32);

impl fmt::Debug for ScopeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "scope#{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Active,
    Disposed,
}

/// An effect's undo, captured at registration. Runs with the tree borrowed
/// so it can remove exactly what it added.
pub type Disposable = Box<dyn FnOnce(&mut ScopeTree) + Send>;

/// A decorator over the current resolution of a name — the concrete shape
/// of "spool A modifies spool B": A wraps B's invocation chain.
pub type Decorate = Box<dyn Fn(Arc<dyn Tool>) -> Arc<dyn Tool> + Send>;

/// Everything a mount is, minus construction: the identity of the body,
/// its mount-time config, its inverse, and the instance to register.
pub struct MountInfo {
    pub name: String,
    pub version: String,
    pub sha256: String,
    pub config: Map<String, Value>,
    pub inverse: Inverse,
    pub instance: Arc<dyn Tool>,
}

type EffectStack = Vec<(ScopeId, Arc<dyn Tool>)>;

#[derive(Debug, Error)]
pub enum ScopeError {
    #[error("scope {0:?} is not active")]
    Dead(ScopeId),
    #[error("cannot unmount {scope}: its {effect} on \"{target}\" is pinned by {blocker}")]
    Pinned {
        scope: String,
        effect: &'static str,
        target: String,
        blocker: String,
    },
    #[error("wrap target is not mounted: {0}")]
    NoTarget(String),
}

/// One disposed node, reported in disposal order (children first) — the
/// caller tapes one Unmount act per entry, ref'd by scope.
#[derive(Debug)]
pub struct Unmounted {
    pub scope: ScopeId,
    pub name: String,
    pub version: String,
    pub sha256: String,
    pub inverse: Inverse,
}

struct Node {
    name: String,
    version: String,
    sha256: String,
    #[allow(dead_code)] // carried for the update path and future acceptors
    config: Map<String, Value>,
    inverse: Inverse,
    parent: Option<ScopeId>,
    children: Vec<ScopeId>,
    disposables: Vec<(String, Disposable)>,
    status: Status,
}

#[derive(Default)]
pub struct ScopeTree {
    // Arena over Rc<RefCell>: node ids stay stable and disposal order is
    // explicit instead of hidden in shared-pointer drop order.
    nodes: Vec<Option<Node>>,
    /// name -> mounted siblings, oldest first; additive, and any of them
    /// may leave first. `resolve` answers with the newest.
    instances: HashMap<String, EffectStack>,
    /// target name -> wrapper chain; each entry already wraps everything
    /// below it, so the top entry is the whole chain.
    wrappers: HashMap<String, EffectStack>,
}

impl ScopeTree {
    pub fn new() -> Self {
        Self::default()
    }

    fn node(&self, id: ScopeId) -> Result<&Node, ScopeError> {
        self.nodes
            .get(id.0 as usize)
            .and_then(Option::as_ref)
            .filter(|n| n.status == Status::Active)
            .ok_or(ScopeError::Dead(id))
    }

    fn node_mut(&mut self, id: ScopeId) -> Result<&mut Node, ScopeError> {
        self.node(id)?;
        self.nodes
            .get_mut(id.0 as usize)
            .and_then(Option::as_mut)
            .ok_or(ScopeError::Dead(id))
    }

    fn name_of(&self, id: ScopeId) -> String {
        self.nodes
            .get(id.0 as usize)
            .and_then(Option::as_ref)
            .map(|n| n.name.clone())
            .unwrap_or_else(|| "<disposed>".into())
    }

    /// Who answers for `name` right now: the newest active sibling with
    /// its wrapper chain. A wrapper pins the composition it was built
    /// over — mounting a newer sibling does not retarget it (v1
    /// documented behavior, not a resolution cache to invalidate).
    pub fn resolve(&self, name: &str) -> Option<Arc<dyn Tool>> {
        if let Some((_, top)) = self.wrappers.get(name).and_then(|w| w.last()) {
            return Some(top.clone());
        }
        self.instances
            .get(name)
            .and_then(|s| s.last())
            .map(|(_, t)| t.clone())
    }

    /// Mount a constructed instance. Its removal from the sibling list is
    /// the scope's first disposable — a mount is reversible by construction.
    pub fn mount(
        &mut self,
        parent: Option<ScopeId>,
        info: MountInfo,
    ) -> Result<ScopeId, ScopeError> {
        if let Some(p) = parent {
            self.node(p)?;
        }
        let id = ScopeId(self.nodes.len() as u32);
        self.nodes.push(Some(Node {
            name: info.name.clone(),
            version: info.version,
            sha256: info.sha256,
            config: info.config,
            inverse: info.inverse,
            parent,
            children: vec![],
            disposables: vec![],
            status: Status::Active,
        }));
        if let Some(p) = parent {
            self.node_mut(p)?.children.push(id);
        }
        let name = info.name;
        self.instances
            .entry(name.clone())
            .or_default()
            .push((id, info.instance));
        self.collect(
            id,
            "instance",
            Box::new(move |tree| {
                if let Some(stack) = tree.instances.get_mut(&name) {
                    stack.retain(|(s, _)| *s != id);
                }
            }),
        )?;
        Ok(id)
    }

    /// Wrap every future resolution of `target` in a decorator built over
    /// the current chain. The disposable removes exactly this layer.
    /// Wrapping is the modification channel, so it pins: the target cannot
    /// leave while wrapped, and a wrapper leaves only from the chain top.
    pub fn wrap(
        &mut self,
        scope: ScopeId,
        target: &str,
        decorate: Decorate,
    ) -> Result<(), ScopeError> {
        self.node(scope)?;
        let inner = self
            .resolve(target)
            .ok_or_else(|| ScopeError::NoTarget(target.to_string()))?;
        let wrapped = decorate(inner);
        self.wrappers
            .entry(target.to_string())
            .or_default()
            .push((scope, wrapped));
        let target = target.to_string();
        self.collect(
            scope,
            format!("wrap {target}"),
            Box::new(move |tree| {
                if let Some(chain) = tree.wrappers.get_mut(&target) {
                    chain.retain(|(s, _)| *s != scope);
                }
            }),
        )
    }

    /// The escape hatch for effects with no dedicated channel: register
    /// the inverse next to the effect, or not at all.
    pub fn collect(
        &mut self,
        scope: ScopeId,
        label: impl Into<String>,
        inverse: Disposable,
    ) -> Result<(), ScopeError> {
        self.node_mut(scope)?
            .disposables
            .push((label.into(), inverse));
        Ok(())
    }

    pub fn status(&self, id: ScopeId) -> Option<Status> {
        self.nodes
            .get(id.0 as usize)
            .and_then(Option::as_ref)
            .map(|n| n.status)
    }

    /// Roots still active — the caller's drop-all sweeps these.
    pub fn active_roots(&self) -> Vec<ScopeId> {
        self.nodes
            .iter()
            .enumerate()
            .filter_map(|(i, n)| {
                let n = n.as_ref()?;
                (n.status == Status::Active && n.parent.is_none()).then_some(ScopeId(i as u32))
            })
            .collect()
    }

    /// Cascade-unmount: children first (newest first — the tree is a
    /// stack), then this scope's disposables in reverse registration
    /// order. Returns every disposed node in disposal order.
    pub fn unmount(&mut self, id: ScopeId) -> Result<Vec<Unmounted>, ScopeError> {
        self.node(id)?;
        let mut report = Vec::new();
        self.dispose_subtree(id, &mut report)?;
        Ok(report)
    }

    fn dispose_subtree(
        &mut self,
        id: ScopeId,
        report: &mut Vec<Unmounted>,
    ) -> Result<(), ScopeError> {
        let children = self.node(id)?.children.clone();
        for child in children.into_iter().rev() {
            self.dispose_subtree(child, report)?;
        }
        self.dispose_node(id, report)
    }

    fn dispose_node(&mut self, id: ScopeId, report: &mut Vec<Unmounted>) -> Result<(), ScopeError> {
        self.check_unpinned(id)?;
        let Some(mut node) = self.nodes.get_mut(id.0 as usize).and_then(Option::take) else {
            return Err(ScopeError::Dead(id));
        };
        while let Some((_, disposable)) = node.disposables.pop() {
            disposable(self);
        }
        if let Some(p) = node.parent
            && let Some(Some(parent)) = self.nodes.get_mut(p.0 as usize)
        {
            parent.children.retain(|c| *c != id);
        }
        report.push(Unmounted {
            scope: id,
            name: node.name,
            version: node.version,
            sha256: node.sha256,
            inverse: node.inverse,
        });
        Ok(())
    }

    /// Stack discipline lives on the modification channel only: a scope
    /// cannot leave while a live wrapper pins it as a target, and a
    /// wrapper leaves only from the top of its chain. Instance siblings
    /// are additive and leave in any order.
    fn check_unpinned(&self, id: ScopeId) -> Result<(), ScopeError> {
        for (name, chain) in &self.wrappers {
            if chain.is_empty() {
                continue;
            }
            // This scope mounted the target while wrappers are live.
            let mounted_target = self
                .instances
                .get(name)
                .is_some_and(|stack| stack.iter().any(|(s, _)| *s == id));
            if mounted_target {
                return Err(ScopeError::Pinned {
                    scope: self.name_of(id),
                    effect: "wrapped target",
                    target: name.clone(),
                    blocker: self.name_of(chain[chain.len() - 1].0),
                });
            }
            // This scope's own wrapper must leave from the top.
            if let Some(pos) = chain.iter().position(|(s, _)| *s == id)
                && pos + 1 != chain.len()
            {
                return Err(ScopeError::Pinned {
                    scope: self.name_of(id),
                    effect: "wrapper",
                    target: name.clone(),
                    blocker: self.name_of(chain[chain.len() - 1].0),
                });
            }
        }
        Ok(())
    }

    /// Bodies are immutable, so any change is a remount in place: cascade
    /// the old subtree, mount the new instance under the same parent and
    /// name. A cordis-style config acceptor (patch without restart) is a
    /// deliberate non-goal for v1 — loosening candidates live in the docs,
    /// not here.
    pub fn update(
        &mut self,
        id: ScopeId,
        mut info: MountInfo,
    ) -> Result<(ScopeId, Vec<Unmounted>), ScopeError> {
        let parent = self.node(id)?.parent;
        info.name = self.node(id)?.name.clone();
        let report = self.unmount(id)?;
        let new_id = self.mount(parent, info)?;
        Ok((new_id, report))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use an_agent_core::act::{ToolCtx, ToolTag};
    use serde_json::json;

    struct Echo(&'static str);

    #[async_trait::async_trait]
    impl Tool for Echo {
        fn name(&self) -> &str {
            self.0
        }

        fn description(&self) -> &str {
            "echo"
        }

        fn parameters(&self) -> Value {
            json!({})
        }

        fn tag_seed(&self) -> Option<ToolTag> {
            Some(ToolTag::none())
        }

        async fn execute(&self, args: Value, _ctx: &ToolCtx) -> Result<String, String> {
            Ok(format!(
                "{}:{}",
                self.0,
                args["text"].as_str().unwrap_or("")
            ))
        }
    }

    fn echo(name: &'static str) -> Arc<dyn Tool> {
        Arc::new(Echo(name))
    }

    fn info(name: &str, version: &str, tool: &'static str) -> MountInfo {
        MountInfo {
            name: name.into(),
            version: version.into(),
            sha256: format!("hash-{name}-{version}"),
            config: Map::new(),
            inverse: Inverse::None,
            instance: echo(tool),
        }
    }

    fn mount(tree: &mut ScopeTree, parent: Option<ScopeId>, name: &str) -> ScopeId {
        let tool = if name == "a" { "a" } else { "b" };
        tree.mount(parent, info(name, "1.0.0", tool)).unwrap()
    }

    async fn say(tool: Arc<dyn Tool>, text: &str) -> String {
        tool.execute(json!({"text": text}), &ToolCtx { signal: None })
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn siblings_share_a_name_and_leave_in_any_order() {
        // Two mounts of one body are two independent forks (koishi-style);
        // resolve answers with the newest, and either may leave first.
        let mut tree = ScopeTree::new();
        let b1 = mount(&mut tree, None, "b");
        let b2 = tree.mount(None, info("b", "1.0.1", "a")).unwrap();
        assert_eq!(say(tree.resolve("b").unwrap(), "hi").await, "a:hi");
        // The older sibling leaves first — no pinning between siblings.
        tree.unmount(b1).unwrap();
        assert_eq!(say(tree.resolve("b").unwrap(), "hi").await, "a:hi");
        tree.unmount(b2).unwrap();
        assert!(tree.resolve("b").is_none());
    }

    #[tokio::test]
    async fn withdrawal_restores_the_pre_wrap_state() {
        // The shape of the claim under test: A wrapped B; once A leaves,
        // what answers is B exactly as before A arrived.
        let mut tree = ScopeTree::new();
        mount(&mut tree, None, "b");
        let a = mount(&mut tree, None, "a");
        tree.wrap(
            a,
            "b",
            Box::new(|inner| Arc::new(Mark("a", inner)) as Arc<dyn Tool>),
        )
        .unwrap();
        assert_eq!(say(tree.resolve("b").unwrap(), "hi").await, "[a]b:hi");
        tree.unmount(a).unwrap();
        assert_eq!(say(tree.resolve("b").unwrap(), "hi").await, "b:hi");
    }

    /// A decorator that truly layers: it delegates to the inner tool and
    /// marks the output, so a test can tell a live chain from a stale one.
    struct Mark(&'static str, Arc<dyn Tool>);

    #[async_trait::async_trait]
    impl Tool for Mark {
        fn name(&self) -> &str {
            self.0
        }

        fn description(&self) -> &str {
            "mark"
        }

        fn parameters(&self) -> Value {
            json!({})
        }

        fn tag_seed(&self) -> Option<ToolTag> {
            Some(ToolTag::none())
        }

        async fn execute(&self, args: Value, ctx: &ToolCtx) -> Result<String, String> {
            Ok(format!("[{}]{}", self.0, self.1.execute(args, ctx).await?))
        }
    }

    #[tokio::test]
    async fn a_wrapped_target_cannot_leave_and_wrappers_leave_from_the_top() {
        let mut tree = ScopeTree::new();
        let b = mount(&mut tree, None, "b");
        let a = mount(&mut tree, None, "a");
        tree.wrap(
            a,
            "b",
            Box::new(|inner| Arc::new(Mark("a", inner)) as Arc<dyn Tool>),
        )
        .unwrap();
        // B is pinned as a wrap target; the error names A.
        let err = tree.unmount(b).unwrap_err();
        assert!(err.to_string().contains("pinned"), "{err}");
        assert_eq!(tree.status(b), Some(Status::Active));
        // A second wrapper makes A's own layer non-top: A cannot leave
        // before C, and the error names C.
        let c = mount(&mut tree, None, "x");
        tree.wrap(
            c,
            "b",
            Box::new(|inner| Arc::new(Mark("c", inner)) as Arc<dyn Tool>),
        )
        .unwrap();
        assert_eq!(say(tree.resolve("b").unwrap(), "hi").await, "[c][a]b:hi");
        let err = tree.unmount(a).unwrap_err();
        assert!(err.to_string().contains("wrapper"), "{err}");
        // Stack order out: C, then A, then B.
        tree.unmount(c).unwrap();
        tree.unmount(a).unwrap();
        tree.unmount(b).unwrap();
        assert!(tree.resolve("b").is_none());
    }

    #[test]
    fn children_leave_before_their_parent_and_disposables_run_lifo() {
        let mut tree = ScopeTree::new();
        let parent = mount(&mut tree, None, "a");
        let child = mount(&mut tree, Some(parent), "b");
        let order = Arc::new(std::sync::Mutex::new(Vec::new()));
        for (scope, label) in [(parent, "p1"), (parent, "p2"), (child, "c1")] {
            let order = order.clone();
            tree.collect(
                scope,
                label,
                Box::new(move |_| order.lock().unwrap().push(label.to_string())),
            )
            .unwrap();
        }
        let report = tree.unmount(parent).unwrap();
        // c1 first (child before parent), then the parent's own two in
        // reverse registration order.
        assert_eq!(*order.lock().unwrap(), vec!["c1", "p2", "p1"]);
        assert_eq!(report.len(), 2);
        assert_eq!(report[0].name, "b");
        assert_eq!(report[1].name, "a");
    }

    #[tokio::test]
    async fn update_remounts_in_place_and_reports_the_old_subtree() {
        let mut tree = ScopeTree::new();
        let old = mount(&mut tree, None, "b");
        let (new, report) = tree.update(old, info("b", "1.1.0", "a")).unwrap();
        assert_eq!(report.len(), 1);
        assert_eq!(report[0].version, "1.0.0");
        assert_eq!(tree.status(old), None);
        assert_eq!(say(tree.resolve("b").unwrap(), "hi").await, "a:hi");
        assert_eq!(tree.status(new), Some(Status::Active));
    }
}
