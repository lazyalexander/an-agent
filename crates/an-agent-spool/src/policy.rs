//! The rhai constructor, policy sort. A published spool body evaluates to
//! a continuation — effect-as-data: the host admits and executes it, the
//! script never calls a model or a tool itself.
//!
//! Salvaged from the retired probes harness. The policy is not a `Tool`
//! and this module is deliberately synchronous: the host drives (and can
//! wrap the call in its own blocking pool or cancellation). A policy
//! declares empty file/net/proc faces — anything it wants done, it yields
//! as a continuation. Its `requires` list is the `invoke_tool` whitelist,
//! enforced by the host, not by this module.

use serde_json::Value;

use an_agent_core::act::MemoryFacet;

use crate::spool::{Constructor, Flow, SpoolSpec};

/// The next step a policy script yields. Malformed output is a hard
/// error, never silently reinterpreted.
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
        args: Value,
    },
    /// Approve proposed call `index` from the invoke effect event `clip`:
    /// the model proposes (taped with full args), the policy disposes by
    /// reference — args never enter the script.
    Approve {
        clip: String,
        index: usize,
    },
}

impl Continuation {
    pub fn from_value(v: &Value) -> Result<Self, String> {
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
            other => Err(format!("unknown continuation kind: {other}")),
        }
    }

    /// Canonical JSON form — what lands on tape as the decision's effect.
    pub fn to_value(&self) -> Value {
        match self {
            Self::Halt => serde_json::json!({ "kind": "halt" }),
            Self::Utter { text } => serde_json::json!({ "kind": "utter", "text": text }),
            Self::InvokeModel { clips } => {
                serde_json::json!({ "kind": "invoke_model", "clips": clips })
            }
            Self::InvokeTool { name, args } => {
                serde_json::json!({ "kind": "invoke_tool", "name": name, "args": args })
            }
            Self::Approve { clip, index } => {
                serde_json::json!({ "kind": "approve", "clip": clip, "index": index })
            }
        }
    }
}

/// A policy body: pure computation over a host-injected tape projection.
/// Construction is the fail-fast gate — a policy declaring file/net/proc
/// effector faces is rejected here, not at first call.
pub struct RhaiPolicy {
    name: String,
    summary: String,
    memory: MemoryFacet,
    /// Names this policy may yield in `invoke_tool` — the spool's
    /// requires list, enforced by the host driver.
    whitelist: Vec<String>,
    /// Declared flow direction. Nothing enforces it yet — the channel it
    /// described is retired with the probes line and regrows from
    /// practice.
    flow: Flow,
    /// Mount config, visible to the script as `config` — one body, many
    /// mounts, different settings.
    config: serde_json::Map<String, Value>,
    script: String,
}

impl RhaiPolicy {
    pub fn from_spool(
        spec: &SpoolSpec,
        config: serde_json::Map<String, Value>,
    ) -> Result<Self, String> {
        use crate::descriptor::{Net, Proc};
        use an_agent_core::act::FileFacet;
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
            Constructor::Rhai { script } => script.clone(),
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

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// The declared memory face. The host reads it to decide whether a
    /// yielded continuation also earns a remember clip.
    pub fn memory(&self) -> &MemoryFacet {
        &self.memory
    }

    pub fn whitelist(&self) -> Vec<String> {
        self.whitelist.clone()
    }

    pub fn flow(&self) -> Flow {
        self.flow
    }

    /// Evaluate the script once over the host-injected projection
    /// (`params`). The result is validated at the boundary: a well-formed
    /// continuation or an error, never garbage.
    pub fn evaluate(&self, args: Value) -> Result<Continuation, String> {
        let value = run_script(&self.script, args, self.config.clone())?;
        Continuation::from_value(&value)
    }
}

/// Spool scripts run untrusted-ish registry content: hard resource
/// bounds, not just an operation budget.
pub(crate) fn bounded_engine() -> rhai::Engine {
    let mut engine = rhai::Engine::new();
    engine.set_max_operations(100_000);
    engine.set_max_string_size(1 << 20);
    engine.set_max_array_size(10_000);
    engine.set_max_map_size(10_000);
    engine.set_max_expr_depths(64, 64);
    engine
}

fn run_script(
    script: &str,
    args: Value,
    config: serde_json::Map<String, Value>,
) -> Result<Value, String> {
    let engine = bounded_engine();
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
    rhai::serde::from_dynamic::<Value>(&out).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests;
