//! Probe-grade mounter: publish a spool, fold its closure against the
//! caller's rights, construct the capability, tape the mount; the returned
//! guard tapes the unmount. Promotion into the kernel is a recorded
//! opening — until then mounting lives here, next to the policy loop.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Map, Value, json};

use an_agent_core::act::{
    ActCtx, ActEnvelope, ActKind, ActSentence, BareFile, Ingest, Permit, Tool,
};
use an_agent_core::memstream::{ActOnEvent, AppendEvent, FromKind, JsonlStore, Kind};
use an_agent_spool::spool::{Closure, Constructor, Faces, Inverse, Registry, SpoolSpec, covers};

use super::AgentError;
use super::rhai::RhaiPolicy;

/// Host constructors the mounter may wire, keyed by the spool's `host:`
/// name. Tests inject fakes here; live runs map "discord" to the reqwest
/// bridge. A spool naming an unregistered host fails at mount.
pub type HostCtors =
    HashMap<String, Box<dyn Fn(&Map<String, Value>) -> Result<Arc<dyn Tool>, String>>>;

/// A mounted spool. Dropping the guard without `unmount` still tapes the
/// unmount — a mount must never outlive its audit trail.
pub struct MountGuard<'a> {
    store: Option<&'a JsonlStore>,
    agent_id: String,
    session: String,
    card: Option<String>,
    spec: SpoolSpec,
    closure: Closure,
    tool: Arc<dyn Tool>,
    /// Set when the mounted spool is a rhai policy — the driver needs the
    /// concrete type for the whitelist and the continuation protocol.
    policy: Option<Arc<RhaiPolicy>>,
    mount_id: Option<String>,
    done: bool,
}

impl MountGuard<'_> {
    pub fn tool(&self) -> Arc<dyn Tool> {
        self.tool.clone()
    }

    pub fn policy(&self) -> Option<Arc<RhaiPolicy>> {
        self.policy.clone()
    }

    pub fn spec(&self) -> &SpoolSpec {
        &self.spec
    }

    pub fn closure(&self) -> &Closure {
        &self.closure
    }

    /// Tape the unmount, refs back to the mount event. An inverse spool is
    /// surfaced, not run: rewinding is the caller's own admitted act, so
    /// the tape shows who decided to rewind and when.
    pub fn unmount(mut self) -> Option<(String, String)> {
        self.tape_unmount();
        match &self.spec.inverse {
            Inverse::Spool { name, version } => Some((name.clone(), version.clone())),
            _ => None,
        }
    }

    fn tape_unmount(&mut self) {
        if self.done {
            return;
        }
        self.done = true;
        let Some(store) = self.store else { return };
        let note = match &self.spec.inverse {
            Inverse::None => "none".to_string(),
            // An irreversible mount unwinds locally only: effects already
            // sent to the world stay there, and the tape says so.
            Inverse::Irreversible => "irreversible".to_string(),
            Inverse::Spool { name, version } => format!("inverse spool surfaced: {name} {version}"),
        };
        let env = ActEnvelope {
            kind: ActKind::Unmount,
            sentence: ActSentence::bare(Permit::Go, BareFile::None, Ingest::Ignore),
            tool: Some(self.spec.name.clone()),
        };
        let _ = store.append(AppendEvent {
            from: self.agent_id.clone(),
            from_kind: FromKind::Agent,
            kind: Kind::Action,
            session: self.session.clone(),
            content: json!({
                "name": self.spec.name,
                "version": self.spec.version,
                "inverse": note,
            })
            .to_string(),
            tags: vec!["spool".into()],
            refs: self.mount_id.iter().cloned().collect(),
            act: Some(ActOnEvent::intent(&env)),
            card: self.card.clone(),
        });
    }
}

impl Drop for MountGuard<'_> {
    fn drop(&mut self) {
        self.tape_unmount();
    }
}

fn tape_mount(
    actx: &ActCtx<'_>,
    spec: &SpoolSpec,
    config: &Map<String, Value>,
    closure: &Closure,
    permit: Permit,
) -> Option<String> {
    let store = actx.store?;
    let env = ActEnvelope {
        kind: ActKind::Mount,
        sentence: ActSentence::bare(permit, BareFile::None, Ingest::Ignore),
        tool: Some(spec.name.clone()),
    };
    let event = store
        .append(AppendEvent {
            from: actx.agent_id.into(),
            from_kind: FromKind::Agent,
            kind: Kind::Action,
            session: actx.session.into(),
            content: json!({
                "name": spec.name,
                "version": spec.version,
                "sha256": spec.sha256,
                "config": config,
                "members": closure.members,
                "reversible": closure.reversible,
                "body": spec.body,
            })
            .to_string(),
            tags: vec!["spool".into(), spec.sha256.clone()],
            refs: vec![],
            act: Some(ActOnEvent::intent(&env)),
            card: actx.card.map(str::to_string),
        })
        .ok()?;
    Some(event.id)
}

/// Publish and mount one spool. Mount-time config is merged over the
/// body's config (mount wins) — one body, many mounts, different settings.
/// The closure ceiling must fit inside `rights`; a refused mount is taped
/// as a Deny before the error returns, so the audit keeps the attempt.
pub fn mount<'a>(
    actx: &ActCtx<'a>,
    registry: &Registry,
    yaml: &str,
    config: Map<String, Value>,
    rights: &Faces,
    hosts: &HostCtors,
) -> Result<MountGuard<'a>, AgentError> {
    let spec = registry
        .publish(yaml)
        .map_err(|e| AgentError::Model(e.to_string()))?;
    let closure = registry
        .closure(&spec.name, &spec.version)
        .map_err(|e| AgentError::Model(e.to_string()))?;
    // Mount-time config merges over the body's config (mount wins) — one
    // body, many mounts, different settings.
    let mut merged = spec.config.clone();
    for (k, v) in &config {
        merged.insert(k.clone(), v.clone());
    }
    if !covers(rights, &closure.ceiling) {
        tape_mount(actx, &spec, &merged, &closure, Permit::Deny);
        return Err(AgentError::Model(format!(
            "mount denied: closure of {} {} exceeds rights",
            spec.name, spec.version
        )));
    }
    let (tool, policy): (Arc<dyn Tool>, Option<Arc<RhaiPolicy>>) = match &spec.constructor {
        // v1 wires rhai spools as policies only — the one rhai sort the
        // probes support.
        Constructor::Rhai { .. } => {
            let policy =
                Arc::new(RhaiPolicy::from_spool(&spec, merged.clone()).map_err(AgentError::Model)?);
            (policy.clone(), Some(policy))
        }
        Constructor::Host { name } => {
            let ctor = hosts.get(name).ok_or_else(|| {
                AgentError::Model(format!("no host constructor registered for {name}"))
            })?;
            (ctor(&merged).map_err(AgentError::Model)?, None)
        }
    };
    let mount_id = tape_mount(actx, &spec, &merged, &closure, Permit::Go);
    Ok(MountGuard {
        store: actx.store,
        agent_id: actx.agent_id.into(),
        session: actx.session.into(),
        card: actx.card.map(str::to_string),
        spec,
        closure,
        tool,
        policy,
        mount_id,
        done: false,
    })
}
