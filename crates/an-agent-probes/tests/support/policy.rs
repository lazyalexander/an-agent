//! Policy loop. A script reads a tape projection and yields a continuation.

use super::step::{admit_invoke_effect, admit_invoke_intent, admit_utterance};
use super::*;

// --- policy-driven loop (control flow lives in a rhai script) ---
//
// The fixed ReAct policy above is one hardcoded policy; here the policy is
// a rhai script mounted on tape. The script sees a tape projection (clip
// ids + previews, not full text) and yields a continuation; the host
// admits and executes it. The loop stays in the host.

use super::rhai::{Continuation, RhaiPolicy};
use an_agent_core::memstream::JsonlStore;

/// Per-step options: the persona prompt (mount-owned, host-injected) and
/// the mounter for silk (admission gate + enhancer chains; None = no silk
/// capability — default deny).
#[derive(Default, Clone, Copy)]
pub struct StepOpts<'a> {
    pub system: Option<&'a str>,
    pub silk: Option<&'a crate::support::mount::Mounter<'a>>,
}

/// How many recent events the policy sees, newest first.
const PROJECTION_TAIL: usize = 20;
const CLIP_PREVIEW_CHARS: usize = 200;

fn ulid() -> String {
    an_agent_core::det_seam::Entropy::os()
        .ulid(an_agent_core::det_seam::Clock::wall().now_ms())
        .to_string()
}

/// Tape the policy descriptor itself (script inline, content-addressed):
/// the audit/reuse source of the control flow about to run.
pub fn mount_policy(
    actx: &an_agent_core::act::ActCtx<'_>,
    name: &str,
    yaml: &str,
) -> Result<Option<String>, AgentError> {
    let hash = an_agent_spool::descriptor::content_hash(yaml);
    let Some(store) = actx.store else {
        return Ok(None);
    };
    let env = ActEnvelope {
        kind: ActKind::Mount,
        permit: Permit::Go,
        tool: Some(name.to_string()),
    };
    store.append(AppendEvent {
        from: actx.agent_id.into(),
        from_kind: FromKind::Agent,
        kind: Kind::Action,
        session: actx.session.into(),
        content: yaml.to_string(),
        tags: vec!["policy".into(), hash.clone()],
        refs: vec![],
        act: Some(ActOnEvent::intent(&env)),
        card: actx.card.map(str::to_string),
    })?;
    Ok(Some(hash))
}

/// Clip ids + metadata + truncated preview, newest first — not full text.
/// The script pulls full content only indirectly, by naming clip ids in
/// `invoke_model` and letting the host assemble the context.
///
/// Isolation (S14): the projection covers the mount's own tape partition
/// plus its silk inbox — envelopes addressed to `alias` cross partitions
/// into the projection, everything else in other partitions does not
/// exist as far as the script is concerned.
fn tape_projection(
    store: &JsonlStore,
    session: &str,
    alias: Option<&str>,
) -> Result<serde_json::Value, AgentError> {
    let events = store.read_all()?;
    let mut clips: Vec<&an_agent_core::memstream::Memevent> = events
        .iter()
        .filter(|e| e.session.as_deref() == Some(session))
        .collect();
    if let Some(alias) = alias {
        for e in events
            .iter()
            .filter(|e| e.session.as_deref() != Some(session))
        {
            let for_me = e.tags.iter().any(|t| t == "silk")
                && serde_json::from_str::<serde_json::Value>(&e.content)
                    .ok()
                    .and_then(|v| v.get("to")?.as_str().map(str::to_string))
                    .as_deref()
                    == Some(alias);
            if for_me {
                clips.push(e);
            }
        }
    }
    // seq is store-global and monotonic; merge by it, newest first.
    clips.sort_by_key(|e| std::cmp::Reverse(e.seq));
    let clips: Vec<serde_json::Value> = clips
        .into_iter()
        .take(PROJECTION_TAIL)
        .map(|e| {
            serde_json::json!({
                "id": e.id,
                "kind": e.kind,
                "from": e.from_kind,
                // The raw principal name (agent id / discord username) —
                // a projection shared by several mounts must let a script
                // tell its own clips apart from another mount's.
                "who": e.from,
                "tags": e.tags,
                "preview": e.content.chars().take(CLIP_PREVIEW_CHARS).collect::<String>(),
            })
        })
        .collect();
    Ok(serde_json::Value::Array(clips))
}

fn last_observation_preview(store: &JsonlStore, session: &str) -> Result<String, AgentError> {
    Ok(store
        .read_all()?
        .iter()
        .rev()
        .find(|e| e.kind == Kind::Observation && e.session.as_deref() == Some(session))
        .map(|e| e.content.chars().take(CLIP_PREVIEW_CHARS).collect())
        .unwrap_or_default())
}

/// Taint (S13): a Human-ingress clip is adversarial by default, and its
/// verbatim content must not feed a world-writing effect (file w/rw /
/// unbounded). The machine enforces the verbatim channel only — a
/// script-authored string goes through the policy body's own
/// responsibility, and the decision args on tape show exactly what it
/// wrote. Reconstruction is the only detaint path, and it is auditable
/// precisely because it passes through a named body or an enhance hop.
fn check_tainted_clip_args(
    store: &JsonlStore,
    session: &str,
    tool_name: &str,
    args: &serde_json::Value,
    world_write: bool,
) -> Result<(), String> {
    if !world_write {
        return Ok(());
    }
    let Some(obj) = args.as_object() else {
        return Ok(());
    };
    let clip_ids: Vec<&str> = obj
        .values()
        .filter_map(|v| v.as_object()?.get("$clip")?.as_str())
        .collect();
    if clip_ids.is_empty() {
        return Ok(());
    }
    let events = store.read_all().map_err(|e| e.to_string())?;
    for id in clip_ids {
        let tainted = events
            .iter()
            .find(|e| e.id == id && e.session.as_deref() == Some(session))
            .is_some_and(|e| e.from_kind == FromKind::Human);
        if tainted {
            return Err(format!(
                "tainted clip {id} (human ingress) cannot feed world-writing tool {tool_name}"
            ));
        }
    }
    Ok(())
}

/// Resolve `{"$clip": "<id>"}` argument values against the tape: the policy
/// only ever sees truncated previews, so when a continuation needs full
/// text (e.g. forwarding a model reply), the host substitutes it from the
/// named clip. One level deep by design — flat argument shapes keep the
/// substitution surface small; clips must belong to this session.
fn resolve_clip_args(
    store: &JsonlStore,
    session: &str,
    args: &mut serde_json::Value,
) -> Result<(), AgentError> {
    let Some(obj) = args.as_object_mut() else {
        return Ok(());
    };
    let needs: bool = obj.values().any(|v| {
        v.as_object()
            .and_then(|m| m.get("$clip"))
            .and_then(|c| c.as_str())
            .is_some()
    });
    if !needs {
        return Ok(());
    }
    let events = store.read_all()?;
    for v in obj.values_mut() {
        let Some(id) = v
            .as_object()
            .and_then(|m| m.get("$clip"))
            .and_then(|c| c.as_str())
            .map(str::to_string)
        else {
            continue;
        };
        let e = events
            .iter()
            .find(|e| e.id == id && e.session.as_deref() == Some(session))
            .ok_or_else(|| AgentError::Model(format!("unknown clip id in args: {id}")))?;
        *v = serde_json::Value::String(e.content.clone());
    }
    Ok(())
}

/// One policy step: project the tape, run the script through admission
/// (the decision itself is taped as intent/effect), execute the
/// continuation. Returns Ok(true) on halt. `system`, when given, is
/// prepended to the context of every invoke_model — a persona belongs to
/// the mount (card side), never to the script body.
pub async fn policy_step(
    actx: &an_agent_core::act::ActCtx<'_>,
    model: &impl Model,
    policy: &Arc<RhaiPolicy>,
    tools: &[Arc<dyn Tool>],
    ctx: &an_agent_core::act::ToolCtx,
    opts: StepOpts<'_>,
) -> Result<bool, AgentError> {
    let (clips, last_obs) = match actx.store {
        Some(store) => (
            tape_projection(store, actx.session, Some(actx.agent_id))?,
            last_observation_preview(store, actx.session)?,
        ),
        None => (serde_json::json!([]), String::new()),
    };
    let args = serde_json::json!({ "clips": clips, "last_observation": last_obs });
    let call = ToolCall {
        id: ulid(),
        name: policy.name().to_string(),
        arguments: args.to_string(),
    };
    let policy_slot = [policy.clone() as Arc<dyn Tool>];
    let decision = an_agent_core::act::run_tool_act(actx, &policy_slot, &call, ctx).await?;
    let value: serde_json::Value = serde_json::from_str(&decision.message.content)
        .map_err(|e| AgentError::Model(format!("policy output is not a continuation: {e}")))?;
    let cont = Continuation::from_value(&value).map_err(AgentError::Model)?;
    match cont {
        Continuation::Halt => Ok(true),
        Continuation::Utter { text } => {
            let _ = admit_utterance(actx, &text)?;
            Ok(false)
        }
        Continuation::InvokeModel { clips } => {
            let store = actx
                .store
                .ok_or_else(|| AgentError::Model("invoke_model needs a store".into()))?;
            // The host assembles context from the named clip ids; the
            // script never touches full text or the model itself.
            let events = store.read_all()?;
            let mut messages = Vec::with_capacity(clips.len() + 1);
            if let Some(system) = opts.system {
                messages.push(ChatMessage {
                    role: "system".into(),
                    content: Some(system.to_string()),
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
            for id in &clips {
                let e = events
                    .iter()
                    .find(|e| &e.id == id)
                    .ok_or_else(|| AgentError::Model(format!("unknown clip id: {id}")))?;
                messages.push(ChatMessage {
                    role: match e.from_kind {
                        FromKind::Agent => "assistant",
                        _ => "user",
                    }
                    .into(),
                    content: Some(e.content.clone()),
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
            let invoke = admit_invoke_intent(actx, model, &messages)?;
            // The model is an oracle, but a bounded one: it only sees the
            // policy's whitelisted tools (requires = static composition at
            // the model surface too). Its proposals land on tape; whether
            // they execute is the policy's approve decision.
            let allowed: Vec<Arc<dyn Tool>> = tools
                .iter()
                .filter(|t| policy.whitelist().iter().any(|n| n == t.name()))
                .cloned()
                .collect();
            let assistant = model.complete(&messages, &allowed).await?;
            admit_invoke_effect(actx, invoke.as_ref(), &assistant)?;
            if !assistant.content.is_empty() {
                let _ = admit_utterance(actx, &assistant.content)?;
            }
            Ok(false)
        }
        Continuation::InvokeTool { name, args } => {
            // Static composition: a policy may only yield tools named in
            // its descriptor's requires; anything else fails hard (the
            // decision is already on tape, the call never happens).
            if !policy.whitelist().iter().any(|n| n == &name) {
                return Err(AgentError::Model(format!(
                    "policy yielded tool outside its requires: {name}"
                )));
            }
            let mut args = args;
            if let Some(store) = actx.store {
                // "Writes the world" is read from the target mount's
                // declared effect when a mounter is in scope, falling back
                // to the tool's admission tag.
                let world_write = match opts.silk.and_then(|m| m.silk_gate().admission(&name)) {
                    Some(adm) => matches!(
                        adm.effect.file,
                        an_agent_core::act::FileFacet::Write { .. }
                            | an_agent_core::act::FileFacet::ReadWrite { .. }
                            | an_agent_core::act::FileFacet::Unbounded
                    ),
                    None => matches!(
                        an_agent_core::act::tag_of(
                            tools.iter().find(|t| t.name() == name).map(|t| t.as_ref())
                        )
                        .file,
                        an_agent_core::act::FileFacet::Write { .. }
                            | an_agent_core::act::FileFacet::ReadWrite { .. }
                            | an_agent_core::act::FileFacet::Unbounded
                    ),
                };
                if let Err(reason) =
                    check_tainted_clip_args(store, actx.session, &name, &args, world_write)
                {
                    // S13 taped like a gate deny: the refusal and its
                    // reason are audit, not silence.
                    store.append(AppendEvent {
                        from: actx.agent_id.into(),
                        from_kind: FromKind::Agent,
                        kind: Kind::Action,
                        session: actx.session.into(),
                        content: serde_json::json!({ "tool": name, "reason": reason }).to_string(),
                        tags: vec!["deny".into(), "taint".into()],
                        refs: vec![],
                        act: None,
                        card: actx.card.map(str::to_string),
                    })?;
                    return Err(AgentError::Model(reason));
                }
                resolve_clip_args(store, actx.session, &mut args)?;
            }
            let call = ToolCall {
                id: ulid(),
                name,
                arguments: args.to_string(),
            };
            let _ = an_agent_core::act::run_tool_act(actx, tools, &call, ctx).await?;
            Ok(false)
        }
        Continuation::Approve { clip, index } => {
            let store = actx
                .store
                .ok_or_else(|| AgentError::Model("approve needs a store".into()))?;
            let events = store.read_all()?;
            let e = events
                .iter()
                .find(|e| e.id == clip && e.session.as_deref() == Some(actx.session))
                .ok_or_else(|| AgentError::Model(format!("unknown clip id: {clip}")))?;
            // Only one's own invoke effects can be quoted: approving from
            // someone else's tape would launder their proposals.
            if e.from != actx.agent_id || e.kind != Kind::Observation {
                return Err(AgentError::Model(format!(
                    "approve must quote an own invoke effect: {clip}"
                )));
            }
            let body: serde_json::Value = serde_json::from_str(&e.content)
                .map_err(|e| AgentError::Model(format!("clip is not an invoke effect: {e}")))?;
            let calls = body
                .get("tool_calls")
                .and_then(|c| c.as_array())
                .ok_or_else(|| AgentError::Model("clip carries no tool_calls".into()))?;
            let proposed = calls
                .get(index)
                .ok_or_else(|| AgentError::Model(format!("no proposed call at index {index}")))?;
            let name = proposed
                .get("name")
                .and_then(|n| n.as_str())
                .ok_or_else(|| AgentError::Model("proposed call has no name".into()))?
                .to_string();
            let arguments = proposed
                .get("arguments")
                .and_then(|a| a.as_str())
                .ok_or_else(|| AgentError::Model("proposed call has no arguments".into()))?
                .to_string();
            // Approval narrows, never widens: the whitelisted-requires
            // check applies exactly as for invoke_tool.
            if !policy.whitelist().iter().any(|n| n == &name) {
                return Err(AgentError::Model(format!(
                    "policy approved a tool outside its requires: {name}"
                )));
            }
            let call = ToolCall {
                id: ulid(),
                name,
                arguments,
            };
            let _ = an_agent_core::act::run_tool_act(actx, tools, &call, ctx).await?;
            Ok(false)
        }
        Continuation::Silk(out) => {
            let store = actx
                .store
                .ok_or_else(|| AgentError::Model("silk needs a store".into()))?;
            // Receiver admission (S12): sender mounted, address resolves,
            // inside the requires closure, flow faces compatible. No gate
            // means no silk capability at all — default deny.
            let mounter = opts
                .silk
                .ok_or_else(|| AgentError::Model("silk needs a mounter".into()))?;
            if let Err(e) = mounter.silk_gate().check(actx.agent_id, &out) {
                // The refusal is taped with its reason: an admission deny
                // is as much audit as the envelopes that pass.
                store.append(AppendEvent {
                    from: actx.agent_id.into(),
                    from_kind: FromKind::Agent,
                    kind: Kind::Action,
                    session: actx.session.into(),
                    content: serde_json::json!({
                        "to": out.to,
                        "kind": out.kind.as_str(),
                        "reason": e.to_string(),
                    })
                    .to_string(),
                    tags: vec!["silk".into(), "deny".into()],
                    refs: vec![],
                    act: None,
                    card: actx.card.map(str::to_string),
                })?;
                return Err(AgentError::Model(e.to_string()));
            }
            // Enhancers on the receiver's inbox transform the payload, in
            // order (S14). Each hop is taped with before/after hashes and
            // linked into the envelope's refs; the stamped fields
            // (from/to/call_id/wake) are never re-read from a transform.
            let mut out = out;
            let mut enhance_refs = vec![];
            for (alias, enhancer) in mounter.enhancer_chain(&out.to) {
                let orig_hash =
                    super::sha256_hex(serde_json::to_vec(&out.payload).unwrap_or_default());
                let envelope_json = serde_json::json!({
                    "kind": out.kind.as_str(),
                    "to": out.to,
                    "from": actx.agent_id,
                    "call_id": out.call_id,
                    "payload": out.payload,
                });
                let new_payload = enhancer
                    .enhance(envelope_json)
                    .map_err(|e| AgentError::Model(format!("enhancer {alias} failed: {e}")))?;
                let new_hash =
                    super::sha256_hex(serde_json::to_vec(&new_payload).unwrap_or_default());
                let link_id = store.append(AppendEvent {
                    from: actx.agent_id.into(),
                    from_kind: FromKind::Agent,
                    kind: Kind::Action,
                    session: actx.session.into(),
                    content: serde_json::json!({
                        "enhancer": alias,
                        "to": out.to,
                        "orig_hash": orig_hash,
                        "new_hash": new_hash,
                    })
                    .to_string(),
                    tags: vec!["silk".into(), "enhance".into()],
                    refs: vec![],
                    act: None,
                    card: actx.card.map(str::to_string),
                })?;
                enhance_refs.push(link_id.id);
                out.payload = new_payload;
            }
            crate::support::silk::deliver_with_refs(
                store,
                actx.agent_id,
                actx.session,
                out,
                enhance_refs,
            )
            .map_err(|e| AgentError::Model(e.to_string()))?;
            Ok(false)
        }
    }
}

pub async fn run_policy_until_idle(
    actx: &an_agent_core::act::ActCtx<'_>,
    model: &impl Model,
    policy: &Arc<RhaiPolicy>,
    tools: &[Arc<dyn Tool>],
    ctx: &an_agent_core::act::ToolCtx,
    max_steps: u32,
    opts: StepOpts<'_>,
) -> Result<(), AgentError> {
    for _ in 0..max_steps {
        if policy_step(actx, model, policy, tools, ctx, opts).await? {
            return Ok(());
        }
    }
    Err(AgentError::Model("policy exceeded maxSteps".into()))
}
