//! Scoped tool registry: registration is an owned handle, not a
//! fire-and-forget insert. Dropping the handle unregisters (RAII), so a
//! tool cannot outlive the scope that registered it — the spatial half of
//! admission ("which tools exist for this agent right now"). The registry
//! itself is pure: taping mount/unmount as acts (`ActKind::Mount` /
//! `Unmount`) belongs to the caller that holds the store.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use thiserror::Error;

use super::Tool;

/// Trust tier of a registered tool — provenance, not capability. The tier
/// says *how* a tool's declared tag is backed, which tells the reader how
/// much weight an audit label carries:
///
/// - `Builtin`: kernel-resident reviewed code (e.g. Bash). Trusted; the
///   tag is an audit label, enforcement is code review.
/// - `Script`: in-process guest gated by its declared effect faces
///   (today: rhai). Semi-trusted; enforcement is mechanical (host-fn
///   wiring).
/// - `Wasm`: sandboxed artifact guest (future). Semi-trusted behind a
///   hard memory boundary.
/// - `External`: out-of-process bridge (MCP). Trust is the process
///   boundary.
///
/// Ordered most-trusted first; `Tier::Builtin < Tier::Script < …`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    Builtin,
    Script,
    Wasm,
    External,
}

#[derive(Debug, Error)]
pub enum RegistryError {
    // Strict: a duplicate name fails instead of replacing — silent shadowing
    // would make the tape cite a different tool than the one admitted.
    #[error("tool already registered: {0}")]
    Duplicate(String),
    #[error("registry lock poisoned")]
    Poisoned,
}

struct Entry {
    tool: Arc<dyn Tool>,
    tier: Tier,
}

/// The live set of tools for one scope. Clone-cheap (shared inner map);
/// every clone sees the same registrations.
#[derive(Clone, Default)]
pub struct ToolRegistry {
    inner: Arc<Mutex<BTreeMap<String, Entry>>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `tool` under `tool.name()` and returns the handle that
    /// owns the registration. Duplicate names fail.
    pub fn register(&self, tool: Arc<dyn Tool>, tier: Tier) -> Result<Registration, RegistryError> {
        let name = tool.name().to_string();
        let mut tools = self.inner.lock().map_err(|_| RegistryError::Poisoned)?;
        if tools.contains_key(&name) {
            return Err(RegistryError::Duplicate(name));
        }
        tools.insert(name.clone(), Entry { tool, tier });
        drop(tools);
        Ok(Registration {
            inner: Arc::clone(&self.inner),
            name,
            armed: true,
        })
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(name)
            .map(|e| e.tool.clone())
    }

    pub fn tier_of(&self, name: &str) -> Option<Tier> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(name)
            .map(|e| e.tier)
    }

    /// Registered names in sorted order — the stable digest input a
    /// topology checkpoint would fold.
    pub fn names(&self) -> Vec<String> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .cloned()
            .collect()
    }

    /// Point-in-time tool list shaped for `run_tool_act`.
    pub fn snapshot(&self) -> Vec<Arc<dyn Tool>> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .map(|e| e.tool.clone())
            .collect()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// RAII handle owning one registration. Dropping it unregisters the tool;
/// `unregister` does the same explicitly and reports whether the entry was
/// still present.
pub struct Registration {
    inner: Arc<Mutex<BTreeMap<String, Entry>>>,
    name: String,
    armed: bool,
}

impl Registration {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn unregister(mut self) -> bool {
        self.armed = false;
        self.remove()
    }

    fn remove(&self) -> bool {
        // A poisoned lock must not strand the registration past its scope.
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.name)
            .is_some()
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        if self.armed {
            self.remove();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::act::ToolCtx;

    struct Echo;

    #[async_trait::async_trait]
    impl Tool for Echo {
        fn name(&self) -> &str {
            "echo"
        }
        fn description(&self) -> &str {
            "echoes args"
        }
        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }
        async fn execute(&self, args: serde_json::Value, _ctx: &ToolCtx) -> Result<String, String> {
            Ok(args.to_string())
        }
    }

    #[test]
    fn register_then_snapshot_contains_tool() {
        let reg = ToolRegistry::new();
        let _guard = reg.register(Arc::new(Echo), Tier::Builtin).unwrap();
        assert_eq!(reg.names(), vec!["echo"]);
        assert_eq!(reg.snapshot().len(), 1);
        assert_eq!(reg.tier_of("echo"), Some(Tier::Builtin));
        assert_eq!(reg.get("echo").unwrap().name(), "echo");
    }

    #[test]
    fn duplicate_name_is_rejected_not_shadowed() {
        let reg = ToolRegistry::new();
        let _guard = reg.register(Arc::new(Echo), Tier::Builtin).unwrap();
        assert!(matches!(
            reg.register(Arc::new(Echo), Tier::Script),
            Err(RegistryError::Duplicate(name)) if name == "echo"
        ));
        assert_eq!(reg.tier_of("echo"), Some(Tier::Builtin));
    }

    #[test]
    fn drop_unregisters() {
        let reg = ToolRegistry::new();
        let guard = reg.register(Arc::new(Echo), Tier::Builtin).unwrap();
        assert_eq!(reg.len(), 1);
        drop(guard);
        assert!(reg.is_empty());
        assert!(reg.get("echo").is_none());
    }

    #[test]
    fn explicit_unregister_reports_presence_once() {
        let reg = ToolRegistry::new();
        let guard = reg.register(Arc::new(Echo), Tier::Builtin).unwrap();
        assert!(guard.unregister());
        assert!(reg.is_empty());
        // Dropping the spent handle must not error or double-remove.
    }

    #[test]
    fn registry_clones_share_scope() {
        let reg = ToolRegistry::new();
        let clone = reg.clone();
        let _guard = reg.register(Arc::new(Echo), Tier::Builtin).unwrap();
        assert_eq!(clone.len(), 1);
    }
}
