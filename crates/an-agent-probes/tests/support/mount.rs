//! Probe-grade mounter over the spool `ScopeTree`: publish, fold the
//! closure, check the ceiling against rights, construct, mount, tape;
//! unmount cascades through the tree and tapes every disposal. Promotion
//! into the kernel is a recorded opening — until then mounting lives here,
//! next to the policy loop.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Map, Value, json};

use an_agent_core::act::{
    ActCtx, ActEnvelope, ActKind, ActSentence, BareFile, Ingest, Permit, Tool,
};
use an_agent_core::memstream::{ActOnEvent, AppendEvent, FromKind, JsonlStore, Kind};
use an_agent_spool::scope::{MountInfo, ScopeId, ScopeTree, Unmounted};
use an_agent_spool::spool::{Constructor, Faces, Inverse, Registry, SpoolSpec, covers};

use super::AgentError;
use super::rhai::RhaiPolicy;

/// Host constructors the mounter may wire, keyed by the spool's `host:`
/// name. Tests inject fakes here; live runs map "discord" to the reqwest
/// bridge. A spool naming an unregistered host fails at mount.
pub type HostCtors =
    HashMap<String, Box<dyn Fn(&Map<String, Value>) -> Result<Arc<dyn Tool>, String>>>;

enum Constructed {
    Tool(Arc<dyn Tool>),
    Policy(Arc<RhaiPolicy>),
}

/// Owns the scope tree and the tape trail for one driver. Dropping the
/// mounter sweeps every remaining root scope — a mount must never outlive
/// its audit trail.
pub struct Mounter<'a> {
    store: Option<&'a JsonlStore>,
    agent_id: String,
    session: String,
    card: Option<String>,
    registry: &'a Registry,
    hosts: HostCtors,
    tree: ScopeTree,
    gate: crate::support::silk::Gate,
    constructed: HashMap<ScopeId, Constructed>,
    mount_events: HashMap<ScopeId, String>,
}

impl<'a> Mounter<'a> {
    pub fn new(actx: &ActCtx<'a>, registry: &'a Registry, hosts: HostCtors) -> Self {
        Self {
            store: actx.store,
            agent_id: actx.agent_id.into(),
            session: actx.session.into(),
            card: actx.card.map(str::to_string),
            registry,
            hosts,
            tree: ScopeTree::new(),
            gate: crate::support::silk::Gate::default(),
            constructed: HashMap::new(),
            mount_events: HashMap::new(),
        }
    }

    pub fn resolve(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tree.resolve(name)
    }

    pub fn policy(&self, id: ScopeId) -> Option<Arc<RhaiPolicy>> {
        match self.constructed.get(&id) {
            Some(Constructed::Policy(p)) => Some(p.clone()),
            _ => None,
        }
    }

    pub fn tree(&self) -> &ScopeTree {
        &self.tree
    }

    pub fn silk_gate(&self) -> &crate::support::silk::Gate {
        &self.gate
    }

    /// Attach an enhancer to a receiver's inbox (S14) and tape the link —
    /// who filters whom is security-relevant configuration and belongs on
    /// the tape.
    pub fn link_enhancer(&mut self, receiver: &str, enhancer: &str) -> Result<(), AgentError> {
        self.gate
            .add_enhancer(receiver, enhancer)
            .map_err(|e| AgentError::Model(e.to_string()))?;
        if let Some(store) = self.store {
            let _ = store.append(AppendEvent {
                from: self.agent_id.clone(),
                from_kind: FromKind::Agent,
                kind: Kind::Action,
                session: self.session.clone(),
                content: json!({ "to": receiver, "enhancer": enhancer }).to_string(),
                tags: vec!["silk".into(), "link".into()],
                refs: vec![],
                act: None,
                card: self.card.clone(),
            });
        }
        Ok(())
    }

    /// The receiver's enhancer chain, resolved to live policies, in
    /// application order.
    pub fn enhancer_chain(&self, receiver: &str) -> Vec<(String, Arc<RhaiPolicy>)> {
        self.gate
            .enhancer_aliases(receiver)
            .iter()
            .filter_map(|alias| {
                let scope = self.gate.admission(alias)?.scope;
                match self.constructed.get(&scope) {
                    Some(Constructed::Policy(p)) => Some((alias.clone(), p.clone())),
                    _ => None,
                }
            })
            .collect()
    }

    /// Publish and mount one spool. Mount-time config is merged over the
    /// body's config (mount wins) — one body, many mounts, different
    /// settings. The closure ceiling must fit inside `rights`; a refused
    /// mount is taped as a Deny before the error returns, so the audit
    /// keeps the attempt. `alias` is the mount's silk address — its tape
    /// identity; default is the spool name, and duplicates are refused
    /// because an ambiguous address is worse than none.
    pub fn mount(
        &mut self,
        parent: Option<ScopeId>,
        yaml: &str,
        config: Map<String, Value>,
        rights: &Faces,
        alias: Option<String>,
        set: Option<String>,
    ) -> Result<ScopeId, AgentError> {
        let spec = self
            .registry
            .publish(yaml)
            .map_err(|e| AgentError::Model(e.to_string()))?;
        let closure = self
            .registry
            .closure(&spec.name, &spec.version)
            .map_err(|e| AgentError::Model(e.to_string()))?;
        let mut merged = spec.config.clone();
        for (k, v) in &config {
            merged.insert(k.clone(), v.clone());
        }
        // Resolved before the rights check so a denial tapes it too.
        let set = set.unwrap_or_else(|| self.session.clone());
        if !covers(rights, &closure.ceiling) {
            self.tape_mount(
                &spec,
                &merged,
                &closure.members,
                closure.reversible,
                Permit::Deny,
                &set,
            );
            return Err(AgentError::Model(format!(
                "mount denied: closure of {} {} exceeds rights",
                spec.name, spec.version
            )));
        }
        // Checked before anything is built: a duplicate alias must fail
        // the mount before the tree changes, not after.
        let alias = alias.unwrap_or_else(|| spec.name.clone());
        if self.gate.admission(&alias).is_some() {
            return Err(AgentError::Model(format!(
                "silk address is already mounted: {alias}"
            )));
        }
        let (tool, constructed) = match &spec.constructor {
            // v1 wires rhai spools as policies only — the one rhai sort
            // the probes support.
            Constructor::Rhai { .. } => {
                let policy = Arc::new(
                    RhaiPolicy::from_spool(&spec, merged.clone()).map_err(AgentError::Model)?,
                );
                let tool: Arc<dyn Tool> = policy.clone();
                (tool, Constructed::Policy(policy))
            }
            Constructor::Host { name } => {
                let ctor = self.hosts.get(name).ok_or_else(|| {
                    AgentError::Model(format!("no host constructor registered for {name}"))
                })?;
                let tool = ctor(&merged).map_err(AgentError::Model)?;
                (tool.clone(), Constructed::Tool(tool))
            }
        };
        let id = self
            .tree
            .mount(
                parent,
                MountInfo {
                    name: spec.name.clone(),
                    version: spec.version.clone(),
                    sha256: spec.sha256.clone(),
                    config: merged.clone(),
                    inverse: spec.inverse.clone(),
                    set: set.clone(),
                    instance: tool,
                },
            )
            .map_err(|e| AgentError::Model(e.to_string()))?;
        self.gate
            .register(
                alias,
                crate::support::silk::Admission {
                    name: spec.name.clone(),
                    closure: closure.members.iter().map(|(n, _)| n.clone()).collect(),
                    flow: spec.effect.flow,
                    effect: spec.effect.clone(),
                    scope: id,
                },
            )
            .map_err(|e| AgentError::Model(e.to_string()))?;
        if let Some(event) = self.tape_mount(
            &spec,
            &merged,
            &closure.members,
            closure.reversible,
            Permit::Go,
            &set,
        ) {
            self.mount_events.insert(id, event);
        }
        self.constructed.insert(id, constructed);
        Ok(id)
    }

    /// Cascade-unmount a scope and tape every disposal, children first.
    /// An inverse spool is surfaced in the report, not run: rewinding is
    /// the caller's own admitted act, so the tape shows who decided.
    pub fn unmount(&mut self, id: ScopeId) -> Result<Vec<Unmounted>, AgentError> {
        let report = self
            .tree
            .unmount(id)
            .map_err(|e| AgentError::Model(e.to_string()))?;
        for entry in &report {
            self.tape_unmount(entry.scope, entry);
            self.gate.unregister(entry.scope);
            self.constructed.remove(&entry.scope);
            self.mount_events.remove(&entry.scope);
        }
        Ok(report)
    }

    fn tape_unmount(&mut self, id: ScopeId, entry: &Unmounted) {
        let Some(store) = self.store else { return };
        let note = match &entry.inverse {
            Inverse::None => "none".to_string(),
            // An irreversible mount unwinds locally only: effects already
            // sent to the world stay there, and the tape says so.
            Inverse::Irreversible => "irreversible".to_string(),
            Inverse::Spool { name, version } => {
                format!("inverse spool surfaced: {name} {version}")
            }
        };
        let env = ActEnvelope {
            kind: ActKind::Unmount,
            sentence: ActSentence::bare(Permit::Go, BareFile::None, Ingest::Ignore),
            tool: Some(entry.name.clone()),
        };
        let _ = store.append(AppendEvent {
            from: self.agent_id.clone(),
            from_kind: FromKind::Agent,
            kind: Kind::Action,
            session: self.session.clone(),
            content: json!({
                "name": entry.name,
                "version": entry.version,
                "inverse": note,
            })
            .to_string(),
            tags: vec!["spool".into()],
            refs: self.mount_events.get(&id).cloned().into_iter().collect(),
            act: Some(ActOnEvent::intent(&env)),
            card: self.card.clone(),
        });
    }

    fn tape_mount(
        &self,
        spec: &SpoolSpec,
        config: &Map<String, Value>,
        members: &[(String, String)],
        reversible: bool,
        permit: Permit,
        set: &str,
    ) -> Option<String> {
        let store = self.store?;
        let env = ActEnvelope {
            kind: ActKind::Mount,
            sentence: ActSentence::bare(permit, BareFile::None, Ingest::Ignore),
            tool: Some(spec.name.clone()),
        };
        let event = store
            .append(AppendEvent {
                from: self.agent_id.clone(),
                from_kind: FromKind::Agent,
                kind: Kind::Action,
                session: self.session.clone(),
                content: json!({
                    "name": spec.name,
                    "version": spec.version,
                    "sha256": spec.sha256,
                    "config": config,
                    "members": members,
                    "reversible": reversible,
                    "set": set,
                    "body": spec.body,
                })
                .to_string(),
                tags: vec!["spool".into(), spec.sha256.clone()],
                refs: vec![],
                act: Some(ActOnEvent::intent(&env)),
                card: self.card.clone(),
            })
            .ok()?;
        Some(event.id)
    }
}

impl Drop for Mounter<'_> {
    fn drop(&mut self) {
        for root in self.tree.active_roots() {
            let _ = self.unmount(root);
        }
    }
}
