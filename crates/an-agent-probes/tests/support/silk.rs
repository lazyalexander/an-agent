//! silk v0 envelopes and delivery (probe-grade). The driver is the only
//! mover of envelopes: a script yields an `OutEnvelope` as a continuation;
//! the driver stamps `from` from the mount's tape identity, checks the
//! termination invariants, and tapes the result. A script that hands us a
//! `from` is malformed — self-asserted sender identity would void the
//! addressing discipline wholesale.
//!
//! Termination: every ask must end with exactly one terminal event — one
//! reply, or one cancel marker — so the tape never holds a dangling
//! question. Cancel is a marker, not an envelope: it addresses nobody.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use an_agent_core::memstream::{AppendEvent, FromKind, JsonlStore, Kind, Memevent};
use an_agent_spool::scope::ScopeId;
use an_agent_spool::spool::{Faces, Flow};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SilkKind {
    Tell,
    Ask,
    Reply,
}

impl SilkKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tell => "tell",
            Self::Ask => "ask",
            Self::Reply => "reply",
        }
    }
}

/// What a script may hand the driver. No `from` (stamped), no `call_id`
/// on ask (minted); `call_id` on reply names the ask it answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutEnvelope {
    pub kind: SilkKind,
    pub to: String,
    pub call_id: Option<String>,
    pub wake: bool,
    pub payload: Value,
}

/// What lands on tape. `origin` is reserved for cross-runtime identity;
/// single-sidecar v0 always writes null.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub v: u32,
    pub kind: SilkKind,
    pub to: String,
    pub from: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    pub wake: bool,
    #[serde(default)]
    pub origin: Option<String>,
    pub payload: Value,
}

#[derive(Debug, Error)]
pub enum SilkError {
    #[error("store: {0}")]
    Store(#[from] an_agent_core::memstream::StoreError),
    #[error("{0}")]
    Reject(String),
}

// --- receiver admission (S12) ---

/// What the gate knows about one mounted alias: its spool name, the names
/// in its requires closure (its address book), its silk direction, and the
/// scope it hangs from. Registered at mount, removed at unmount — presence
/// in the gate means the mount is live.
#[derive(Debug, Clone)]
pub struct Admission {
    pub name: String,
    pub closure: std::collections::HashSet<String>,
    pub flow: Flow,
    /// The mount's declared effect — the taint check reads "writes the
    /// world" from this declaration, never from a tool's good behavior.
    pub effect: Faces,
    pub scope: ScopeId,
}

/// The receiver-admission gate. The requires graph is the trust graph,
/// and this is where that sentence becomes real: before any envelope is
/// taped, the gate checks that the sender is a live mount, the address
/// resolves to a live mount, the receiver's spool name sits in the
/// sender's requires closure, and the two flow faces are compatible.
///
/// The gate also holds the enhancer chains (S14): receiver-attached
/// transforms that every inbound envelope passes through. An enhancer
/// must be in the receiver's own requires closure and declare flow both
/// (it receives the envelope and its output continues the send).
#[derive(Debug, Default)]
pub struct Gate {
    by_alias: std::collections::HashMap<String, Admission>,
    by_scope: std::collections::HashMap<ScopeId, String>,
    /// receiver alias -> enhancer aliases, in application order.
    enhancers: std::collections::HashMap<String, Vec<String>>,
}

impl Gate {
    pub fn register(&mut self, alias: String, admission: Admission) -> Result<(), SilkError> {
        if self.by_alias.contains_key(&alias) {
            return Err(reject(format!("silk address is already mounted: {alias}")));
        }
        self.by_scope.insert(admission.scope, alias.clone());
        self.by_alias.insert(alias, admission);
        Ok(())
    }

    pub fn unregister(&mut self, scope: ScopeId) {
        if let Some(alias) = self.by_scope.remove(&scope) {
            self.by_alias.remove(&alias);
            // An unmounted enhancer leaves every chain; an unmounted
            // receiver's chain goes with it.
            self.enhancers.remove(&alias);
            for chain in self.enhancers.values_mut() {
                chain.retain(|a| a != &alias);
            }
        }
    }

    pub fn admission(&self, alias: &str) -> Option<&Admission> {
        self.by_alias.get(alias)
    }

    /// Attach an enhancer to a receiver's inbox. The receiver delegates
    /// its filtering, so the enhancer must sit in the receiver's closure;
    /// an enhancer sees envelopes and re-emits them, so its flow is both.
    pub fn add_enhancer(&mut self, receiver: &str, enhancer: &str) -> Result<(), SilkError> {
        let Some(r) = self.by_alias.get(receiver) else {
            return Err(reject(format!("no such silk address: {receiver}")));
        };
        let Some(e) = self.by_alias.get(enhancer) else {
            return Err(reject(format!(
                "enhancer is not a mounted silk address: {enhancer}"
            )));
        };
        if !r.closure.contains(&e.name) {
            return Err(reject(format!(
                "enhancer {enhancer} is outside the receiver's requires closure: {receiver}"
            )));
        }
        if e.flow != Flow::Both {
            return Err(reject(format!(
                "enhancer {enhancer} must declare flow both, has {:?}",
                e.flow
            )));
        }
        let chain = self.enhancers.entry(receiver.to_string()).or_default();
        if chain.iter().any(|a| a == enhancer) {
            return Err(reject(format!("enhancer {enhancer} already on {receiver}")));
        }
        chain.push(enhancer.to_string());
        Ok(())
    }

    pub fn enhancer_aliases(&self, receiver: &str) -> &[String] {
        self.enhancers
            .get(receiver)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// The three checks, in order. A miss names its reason; the caller
    /// tapes the attempt and drops the envelope.
    pub fn check(&self, sender: &str, out: &OutEnvelope) -> Result<(), SilkError> {
        let Some(sender_adm) = self.by_alias.get(sender) else {
            return Err(reject(format!(
                "sender is not a mounted silk address: {sender}"
            )));
        };
        let Some(receiver_adm) = self.by_alias.get(&out.to) else {
            return Err(reject(format!("no such silk address: {}", out.to)));
        };
        if !sender_adm.closure.contains(&receiver_adm.name) {
            return Err(reject(format!(
                "{} is outside the sender's requires closure: {}",
                out.to, sender
            )));
        }
        if !matches!(sender_adm.flow, Flow::Out | Flow::Both) {
            return Err(reject(format!(
                "{sender} has flow {:?} and may not send silk",
                sender_adm.flow
            )));
        }
        if !matches!(receiver_adm.flow, Flow::In | Flow::Both) {
            return Err(reject(format!(
                "{} has flow {:?} and may not receive silk",
                out.to, receiver_adm.flow
            )));
        }
        Ok(())
    }
}

fn reject(msg: impl Into<String>) -> SilkError {
    SilkError::Reject(msg.into())
}

fn ulid() -> String {
    an_agent_core::det_seam::Entropy::os()
        .ulid(an_agent_core::det_seam::Clock::wall().now_ms())
        .to_string()
}

fn silk_events(store: &JsonlStore) -> Result<Vec<Memevent>, SilkError> {
    Ok(store
        .read_all()?
        .into_iter()
        .filter(|e| e.tags.iter().any(|t| t == "silk"))
        .collect())
}

fn envelope_of(event: &Memevent) -> Option<Envelope> {
    serde_json::from_str(&event.content).ok()
}

fn find_ask<'a>(events: &'a [Memevent], call_id: &str) -> Option<(&'a Memevent, Envelope)> {
    events.iter().find_map(|e| {
        let env = envelope_of(e)?;
        (env.kind == SilkKind::Ask && env.call_id.as_deref() == Some(call_id)).then_some((e, env))
    })
}

/// A call is terminated by a reply carrying its call_id, or by a cancel
/// marker (tags silk+cancel, content names the call_id).
fn has_terminal(events: &[Memevent], call_id: &str) -> bool {
    events.iter().any(|e| {
        if e.tags.iter().any(|t| t == "cancel") {
            return serde_json::from_str::<Value>(&e.content)
                .ok()
                .and_then(|v| v["call_id"].as_str().map(str::to_string))
                .as_deref()
                == Some(call_id);
        }
        envelope_of(e).is_some_and(|env| {
            env.kind == SilkKind::Reply && env.call_id.as_deref() == Some(call_id)
        })
    })
}

fn tape(
    store: &JsonlStore,
    from: &str,
    session: &str,
    tags: Vec<String>,
    content: String,
    refs: Vec<String>,
) -> Result<String, SilkError> {
    let event = store.append(AppendEvent {
        from: from.into(),
        from_kind: FromKind::Agent,
        kind: Kind::Action,
        // v0 rides a single tape partition; cross-partition delivery is
        // the record_both generalization, not yet needed.
        session: session.into(),
        content,
        tags,
        refs,
        act: None,
        card: None,
    })?;
    Ok(event.id)
}

/// Stamp and tape an outgoing envelope. Returns the taped event id.
/// Rejections return Err; the caller's own decision act is already on
/// tape, so the attempt is never invisible.
pub fn deliver(
    store: &JsonlStore,
    sender: &str,
    session: &str,
    out: OutEnvelope,
) -> Result<String, SilkError> {
    deliver_with_refs(store, sender, session, out, vec![])
}

/// `extra_refs` links the envelope to what shaped it in transit — the
/// enhance link events of the receiver's chain (S14).
pub fn deliver_with_refs(
    store: &JsonlStore,
    sender: &str,
    session: &str,
    out: OutEnvelope,
    extra_refs: Vec<String>,
) -> Result<String, SilkError> {
    if out.to.trim().is_empty() {
        return Err(reject("envelope requires a non-empty to"));
    }
    let events = silk_events(store)?;
    let mut refs = extra_refs;
    let call_id = match out.kind {
        SilkKind::Tell => {
            if out.call_id.is_some() {
                return Err(reject("tell must not carry a call_id"));
            }
            None
        }
        SilkKind::Ask => {
            if out.call_id.is_some() {
                return Err(reject("ask mints its own call_id"));
            }
            Some(ulid())
        }
        SilkKind::Reply => {
            let id = out
                .call_id
                .ok_or_else(|| reject("reply requires a call_id"))?;
            let Some((ask, _)) = find_ask(&events, &id) else {
                return Err(reject(format!("reply to unknown call_id: {id}")));
            };
            if has_terminal(&events, &id) {
                return Err(reject(format!("call {id} is already terminated")));
            }
            refs.push(ask.id.clone());
            Some(id)
        }
    };
    let env = Envelope {
        v: 0,
        kind: out.kind,
        to: out.to,
        from: sender.to_string(),
        call_id,
        wake: out.wake,
        origin: None,
        payload: out.payload,
    };
    let content = serde_json::to_string(&env).map_err(|e| reject(e.to_string()))?;
    tape(
        store,
        sender,
        session,
        vec!["silk".into(), env.kind.as_str().into()],
        content,
        refs,
    )
}

/// Terminate an open ask without a reply. Only the asker may cancel (the
/// driver's abort path acts on the asker's behalf, same identity). The
/// marker is not an envelope: it addresses nobody.
pub fn cancel_ask(
    store: &JsonlStore,
    who: &str,
    session: &str,
    call_id: &str,
    reason: &str,
) -> Result<String, SilkError> {
    let events = silk_events(store)?;
    let Some((ask, env)) = find_ask(&events, call_id) else {
        return Err(reject(format!("cancel of unknown call_id: {call_id}")));
    };
    if env.from != who {
        return Err(reject(format!("only the asker may cancel {call_id}")));
    }
    if has_terminal(&events, call_id) {
        return Err(reject(format!("call {call_id} is already terminated")));
    }
    tape(
        store,
        who,
        session,
        vec!["silk".into(), "cancel".into()],
        serde_json::json!({"call_id": call_id, "reason": reason}).to_string(),
        vec![ask.id.clone()],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::support::TempDir;

    fn tell(to: &str) -> OutEnvelope {
        OutEnvelope {
            kind: SilkKind::Tell,
            to: to.into(),
            call_id: None,
            wake: false,
            payload: Value::Null,
        }
    }

    fn ask(to: &str, wake: bool) -> OutEnvelope {
        OutEnvelope {
            kind: SilkKind::Ask,
            to: to.into(),
            call_id: None,
            wake,
            payload: Value::Null,
        }
    }

    fn reply(to: &str, call_id: &str) -> OutEnvelope {
        OutEnvelope {
            kind: SilkKind::Reply,
            to: to.into(),
            call_id: Some(call_id.into()),
            wake: false,
            payload: Value::Null,
        }
    }

    const S: &str = "s1";

    #[test]
    fn ask_reply_refs_and_single_termination() {
        let tmp = TempDir::new("silk");
        let store = JsonlStore::open(tmp.path().join("memory.jsonl")).unwrap();
        let ask_id = deliver(&store, "ada", S, ask("bob", true)).unwrap();
        let events = silk_events(&store).unwrap();
        let env = envelope_of(events.last().unwrap()).unwrap();
        // Stamped, not claimed: from is the delivering identity, wake rides
        // the envelope, and origin stays null on a single sidecar.
        assert_eq!(env.from, "ada");
        assert!(env.wake);
        assert_eq!(env.origin, None);
        assert_eq!(events.last().unwrap().session.as_deref(), Some(S));
        let call_id = env.call_id.clone().unwrap();

        let reply_id = deliver(&store, "bob", S, reply("ada", &call_id)).unwrap();
        let events = silk_events(&store).unwrap();
        let reply_event = events.iter().find(|e| e.id == reply_id).unwrap();
        assert_eq!(reply_event.refs, vec![ask_id]);

        // A second terminal is refused, whether reply or cancel.
        let second = deliver(&store, "bob", S, reply("ada", &call_id));
        assert!(second.unwrap_err().to_string().contains("terminated"));
        assert!(
            cancel_ask(&store, "ada", S, &call_id, "too late")
                .unwrap_err()
                .to_string()
                .contains("terminated")
        );
    }

    #[test]
    fn reply_to_unknown_and_cancel_by_stranger_are_rejected() {
        let tmp = TempDir::new("silk-reject");
        let store = JsonlStore::open(tmp.path().join("memory.jsonl")).unwrap();
        let unknown = deliver(&store, "ada", S, reply("bob", "nope"));
        assert!(unknown.unwrap_err().to_string().contains("unknown call_id"));

        deliver(&store, "ada", S, ask("bob", false)).unwrap();
        let events = silk_events(&store).unwrap();
        let call_id = envelope_of(events.last().unwrap())
            .unwrap()
            .call_id
            .unwrap();
        assert!(
            cancel_ask(&store, "eve", S, &call_id, "malice")
                .unwrap_err()
                .to_string()
                .contains("asker")
        );
        cancel_ask(&store, "ada", S, &call_id, "gave up").unwrap();
        assert!(
            cancel_ask(&store, "ada", S, &call_id, "again")
                .unwrap_err()
                .to_string()
                .contains("terminated")
        );
    }

    #[test]
    fn malformed_shapes_are_rejected() {
        let tmp = TempDir::new("silk-shape");
        let store = JsonlStore::open(tmp.path().join("memory.jsonl")).unwrap();
        assert!(
            deliver(&store, "ada", S, tell("  "))
                .unwrap_err()
                .to_string()
                .contains("to")
        );
        let tell_with_id = OutEnvelope {
            call_id: Some("x".into()),
            ..tell("bob")
        };
        assert!(
            deliver(&store, "ada", S, tell_with_id)
                .unwrap_err()
                .to_string()
                .contains("call_id")
        );
    }
}
