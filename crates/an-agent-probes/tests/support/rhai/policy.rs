//! A policy script yields a continuation. The host admits and runs it.

use super::*;

// --- policy sort: tape projection in, continuation out ---

/// The next step a policy script yields (effect-as-data): the host admits
/// and executes it; the script never calls a model or a tool itself.
/// Malformed output is a hard error, never silently reinterpreted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Continuation {
    Halt,
    Utter {
        text: String,
    },
    InvokeModel {
        clips: Vec<String>,
    },
    InvokeTool {
        name: String,
        args: serde_json::Value,
    },
    /// Approve proposed call `index` from the invoke effect event `clip`:
    /// the model proposes (taped with full args), the policy disposes by
    /// reference — args never enter the script. Approval is the policy's
    /// checkpoint: it lifts taint the way reconstruction does, with the
    /// whole propose→approve→execute chain on tape.
    Approve {
        clip: String,
        index: usize,
    },
    /// A silk envelope handed to the driver: kind/to/payload (+ wake,
    /// + call_id for reply). Never carries `from` — the driver stamps it.
    Silk(crate::support::silk::OutEnvelope),
}

impl Continuation {
    pub fn from_value(v: &serde_json::Value) -> Result<Self, String> {
        let kind = v
            .get("kind")
            .and_then(|k| k.as_str())
            .ok_or("continuation requires a string kind")?;
        match kind {
            "halt" => Ok(Self::Halt),
            "utter" => Ok(Self::Utter {
                text: v
                    .get("text")
                    .and_then(|t| t.as_str())
                    .ok_or("utter requires a string text")?
                    .to_string(),
            }),
            "invoke_model" => {
                let clips = v
                    .get("clips")
                    .and_then(|c| c.as_array())
                    .ok_or("invoke_model requires a clips array")?
                    .iter()
                    .map(|c| {
                        c.as_str()
                            .map(str::to_string)
                            .ok_or_else(|| "clips entries must be strings".to_string())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Self::InvokeModel { clips })
            }
            "invoke_tool" => {
                let name = v
                    .get("name")
                    .and_then(|n| n.as_str())
                    .ok_or("invoke_tool requires a string name")?
                    .to_string();
                let args = v
                    .get("args")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({}));
                if !args.is_object() {
                    return Err("invoke_tool args must be an object".into());
                }
                Ok(Self::InvokeTool { name, args })
            }
            "approve" => {
                let clip = v
                    .get("clip")
                    .and_then(|c| c.as_str())
                    .ok_or("approve requires a string clip")?
                    .to_string();
                let index = v
                    .get("index")
                    .and_then(|i| i.as_u64())
                    .ok_or("approve requires a non-negative integer index")?
                    as usize;
                Ok(Self::Approve { clip, index })
            }
            "silk" => {
                let inner = v
                    .get("silk")
                    .ok_or("silk continuation requires a silk object")?;
                // Stamped, not claimed: a script naming its own sender is
                // malformed, never silently corrected.
                if inner.get("from").is_some() {
                    return Err("silk envelope must not carry from: the driver stamps it".into());
                }
                let kind = match inner
                    .get("kind")
                    .and_then(|k| k.as_str())
                    .ok_or("silk requires a string kind")?
                {
                    "tell" => crate::support::silk::SilkKind::Tell,
                    "ask" => crate::support::silk::SilkKind::Ask,
                    "reply" => crate::support::silk::SilkKind::Reply,
                    other => return Err(format!("unknown silk kind: {other}")),
                };
                let to = inner
                    .get("to")
                    .and_then(|t| t.as_str())
                    .ok_or("silk requires a string to")?
                    .to_string();
                let call_id = inner
                    .get("call_id")
                    .and_then(|c| c.as_str())
                    .map(str::to_string);
                let wake = inner.get("wake").and_then(|w| w.as_bool()).unwrap_or(false);
                let payload = inner
                    .get("payload")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                Ok(Self::Silk(crate::support::silk::OutEnvelope {
                    kind,
                    to,
                    call_id,
                    wake,
                    payload,
                }))
            }
            other => Err(format!("unknown continuation kind: {other}")),
        }
    }

    /// Canonical JSON form — what lands on tape as the decision's effect.
    pub fn to_value(&self) -> serde_json::Value {
        match self {
            Self::Halt => serde_json::json!({ "kind": "halt" }),
            Self::Utter { text } => serde_json::json!({ "kind": "utter", "text": text }),
            Self::InvokeModel { clips } => {
                serde_json::json!({ "kind": "invoke_model", "clips": clips })
            }
            Self::InvokeTool { name, args } => {
                serde_json::json!({ "kind": "invoke_tool", "name": name, "args": args })
            }
            Self::Silk(out) => {
                let mut inner = serde_json::json!({
                    "kind": out.kind.as_str(),
                    "to": out.to,
                    "wake": out.wake,
                    "payload": out.payload,
                });
                if let Some(id) = &out.call_id {
                    inner["call_id"] = serde_json::Value::String(id.clone());
                }
                serde_json::json!({ "kind": "silk", "silk": inner })
            }
            Self::Approve { clip, index } => {
                serde_json::json!({ "kind": "approve", "clip": clip, "index": index })
            }
        }
    }
}

/// Policy-sort rhai tool: pure computation over a host-injected tape
/// projection. The declared effect must be empty of effectors — a policy
/// that declares file/net/proc faces is rejected at construction, the
/// same fail-fast gate as RhaiTool.
pub struct RhaiPolicy {
    name: String,
    summary: String,
    memory: an_agent_core::act::MemoryFacet,
    /// Names this policy may yield in `invoke_tool` — the descriptor's
    /// requires list, enforced by the host driver.
    whitelist: Vec<String>,
    /// Silk direction. The flow face is the policy's silk capability:
    /// out/both may send envelopes, in/both receive them via projection,
    /// none does neither. Legacy descriptors predate the face and get
    /// none — strictest, and they never spoke silk.
    flow: an_agent_spool::spool::Flow,
    /// Mount config, visible to the script as `config` — one body, many
    /// mounts, different settings (e.g. a character's mention handle).
    config: serde_json::Map<String, serde_json::Value>,
    script: String,
}

impl RhaiPolicy {
    pub fn from_descriptor(desc: Descriptor) -> Result<Self, String> {
        use an_agent_core::act::FileFacet;
        use an_agent_spool::descriptor::{Net, Proc};
        if desc.effect.file != FileFacet::None {
            return Err(format!(
                "policy declares a file face it cannot use: {:?}",
                desc.effect.file
            ));
        }
        if desc.effect.net != Net::None {
            return Err("policy declares net it cannot use".into());
        }
        if desc.effect.proc_ != Proc::None {
            return Err("policy declares proc it cannot use".into());
        }
        let script = match &desc.constructor {
            Constructor::Rhai { script } => script.clone(),
            _ => return Err("not a rhai descriptor".into()),
        };
        Ok(Self {
            name: desc.name,
            summary: desc.summary,
            memory: desc.effect.memory,
            whitelist: desc.requires.iter().map(|r| r.name.clone()).collect(),
            flow: an_agent_spool::spool::Flow::None,
            config: serde_json::Map::new(),
            script,
        })
    }

    /// Same gate from a published spool: file/net/proc must be empty — a
    /// policy is pure computation over a projection; its only way to touch
    /// anything is yielding continuations. The flow face is allowed and
    /// kept: it is the policy's silk capability, enforced at delivery.
    pub fn from_spool(
        spec: &an_agent_spool::spool::SpoolSpec,
        config: serde_json::Map<String, serde_json::Value>,
    ) -> Result<Self, String> {
        use an_agent_core::act::FileFacet;
        use an_agent_spool::descriptor::{Net, Proc};
        use an_agent_spool::spool::Constructor as SpoolCtor;
        if spec.effect.file != FileFacet::None {
            return Err(format!(
                "policy declares a file face it cannot use: {:?}",
                spec.effect.file
            ));
        }
        if spec.effect.net != Net::None {
            return Err("policy declares net it cannot use".into());
        }
        if spec.effect.proc_ != Proc::None {
            return Err("policy declares proc it cannot use".into());
        }
        let script = match &spec.constructor {
            SpoolCtor::Rhai { script } => script.clone(),
            _ => return Err("not a rhai spool".into()),
        };
        Ok(Self {
            name: spec.name.clone(),
            summary: spec.summary.clone(),
            memory: spec.effect.memory.clone(),
            whitelist: spec.requires.iter().map(|r| r.name.clone()).collect(),
            flow: spec.effect.flow,
            config,
            script,
        })
    }

    pub fn whitelist(&self) -> Vec<String> {
        self.whitelist.clone()
    }

    pub fn flow(&self) -> an_agent_spool::spool::Flow {
        self.flow
    }

    /// The enhancer entry (S14): the script sees `params.envelope` and
    /// returns the new payload — a transform, not a continuation. The
    /// stamped fields never re-enter: the caller keeps from/to/call_id
    /// and swaps only the payload.
    pub fn enhance(&self, envelope: serde_json::Value) -> Result<serde_json::Value, String> {
        run_policy_script(
            &self.script,
            serde_json::json!({ "envelope": envelope }),
            self.config.clone(),
        )
    }
}

fn run_policy_script(
    script: &str,
    args: serde_json::Value,
    config: serde_json::Map<String, serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let engine = super::bounded_engine();
    let mut scope = rhai::Scope::new();
    scope.push(
        "params",
        rhai::serde::to_dynamic(args).map_err(|e| e.to_string())?,
    );
    scope.push(
        "config",
        rhai::serde::to_dynamic(config).map_err(|e| e.to_string())?,
    );
    let out = engine
        .eval_with_scope::<rhai::Dynamic>(&mut scope, script)
        .map_err(|e| e.to_string())?;
    rhai::serde::from_dynamic::<serde_json::Value>(&out).map_err(|e| e.to_string())
}

#[async_trait::async_trait]
impl Tool for RhaiPolicy {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.summary
    }

    fn parameters(&self) -> serde_json::Value {
        // Not model-facing: the host injects the tape projection as args.
        serde_json::json!({
            "type": "object",
            "properties": {
                "clips": {
                    "type": "array",
                    "description": "recent tape projection, newest first: id/kind/from/tags/preview per clip"
                },
                "last_observation": { "type": "string" }
            }
        })
    }

    fn tag_seed(&self) -> Option<ToolTag> {
        Some(ToolTag {
            file: an_agent_core::act::FileFacet::None,
            permit: Permit::Go,
            memory: self.memory.clone(),
        })
    }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolCtx) -> Result<String, String> {
        let script = self.script.clone();
        let config = self.config.clone();
        let mut task =
            tokio::task::spawn_blocking(move || run_policy_script(&script, args, config));
        let value = tokio::select! {
            res = &mut task => res.map_err(|e| e.to_string())??,
            _ = super::wait_cancel(ctx.signal.clone()) => return Err("cancelled".into()),
        };
        // Validate at the boundary: the taped effect is always a
        // well-formed continuation or an error, never garbage.
        let cont = Continuation::from_value(&value)?;
        Ok(cont.to_value().to_string())
    }
}
