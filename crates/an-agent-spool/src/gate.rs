//! The rhai constructor, gate sort. A gate is a hook handler: pure
//! computation over the delivery subject, verdict out. Same sandbox as
//! the beat sort — bounded engine, empty effector faces enforced at
//! construction. v1 is the gate gear only (allow / deny); the transform
//! gear (rewrite, both versions taped) is a later slice.
//!
//! The script sees `subject` (the event for the before chain, the reply
//! for the after chain — both maps) and `chain` ("before" / "after"),
//! and must return `true` / `false` or `#{ allow: bool, reason: "..." }`.
//! Any other return is an error, and a hook error reads as deny:
//! fail-closed.

use serde_json::Value;

use an_agent_core::control::HookVerdict;

use crate::spool::{Constructor, SpoolSpec};

pub struct RhaiGate {
    engine: rhai::Engine,
    ast: rhai::AST,
}

impl RhaiGate {
    /// Build a gate from a published body. Construction is the fail-fast
    /// gate, same discipline as the beat sort: rhai constructor, empty
    /// effector faces, and the script compiles now, not at the first
    /// delivery.
    pub fn from_spool(spec: &SpoolSpec) -> Result<Self, String> {
        crate::beat::require_effectless(spec)?;
        let script = match &spec.constructor {
            Constructor::Rhai { script } => script,
            _ => return Err("not a rhai spool".into()),
        };
        let engine = crate::policy::bounded_engine();
        let ast = engine.compile(script).map_err(|e| e.to_string())?;
        Ok(Self { engine, ast })
    }

    /// One verdict. `subject` is the chain's subject as a JSON map; the
    /// script's return is read as: `true` → allow, `false` → deny,
    /// `#{ allow, reason? }` → either with the reason taped on a deny.
    pub fn judge(&self, chain: &str, subject: Value) -> Result<HookVerdict, String> {
        let mut scope = rhai::Scope::new();
        scope.push(
            "subject",
            rhai::serde::to_dynamic(subject).map_err(|e| e.to_string())?,
        );
        scope.push("chain", chain.to_string());
        let out = self
            .engine
            .eval_ast_with_scope::<rhai::Dynamic>(&mut scope, &self.ast)
            .map_err(|e| e.to_string())?;
        if let Ok(allow) = out.as_bool() {
            return Ok(if allow {
                HookVerdict::Allow
            } else {
                HookVerdict::Deny {
                    reason: "gate returned false".into(),
                }
            });
        }
        if out.is_map() {
            let value: Value = rhai::serde::from_dynamic(&out).map_err(|e| e.to_string())?;
            let allow = value
                .get("allow")
                .and_then(Value::as_bool)
                .ok_or("gate map needs allow: bool")?;
            return Ok(if allow {
                HookVerdict::Allow
            } else {
                HookVerdict::Deny {
                    reason: value
                        .get("reason")
                        .and_then(Value::as_str)
                        .unwrap_or("denied")
                        .to_string(),
                }
            });
        }
        Err("gate must return bool or #{ allow, reason }".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spool;

    fn spec(script: &str) -> SpoolSpec {
        let yaml = format!(
            "v: 1\nkind: spool\nname: g\nversion: 1.0.0\nsummary: g\nconstructor: rhai\nscript: |\n  {script}\neffect:\n  net: none\n  file: {{ op: none }}\n  proc: none\n  memory: {{ op: ignore }}\n  flow: none\ninverse: none\nrequires: []\nconsumes: []\nproduces: []\n"
        );
        spool::parse(&yaml).unwrap()
    }

    #[test]
    fn construction_is_the_fail_fast_gate() {
        assert!(RhaiGate::from_spool(&spec("let x = ;")).is_err());
        let host = spool::parse(
            "v: 1\nkind: spool\nname: h\nversion: 1.0.0\nsummary: h\nconstructor: host\nhost: echo\neffect:\n  net: none\n  file: { op: none }\n  proc: none\n  memory: { op: ignore }\n  flow: none\ninverse: none\nrequires: []\nconsumes: []\nproduces: []\n",
        )
        .unwrap();
        assert!(RhaiGate::from_spool(&host).is_err());
        let mut faced = spec("true");
        faced.effect.file = an_agent_core::act::FileFacet::Unbounded;
        assert!(RhaiGate::from_spool(&faced).is_err());
    }

    #[test]
    fn the_return_reads_as_a_verdict() {
        let subject = serde_json::json!({"body": "x"});
        let allow = RhaiGate::from_spool(&spec("true")).unwrap();
        assert_eq!(
            allow.judge("before", subject.clone()).unwrap(),
            HookVerdict::Allow
        );
        let deny = RhaiGate::from_spool(&spec("false")).unwrap();
        assert!(matches!(
            deny.judge("before", subject.clone()).unwrap(),
            HookVerdict::Deny { .. }
        ));
        let reasoned = RhaiGate::from_spool(&spec("#{ allow: false, reason: \"no\" }")).unwrap();
        assert!(matches!(
            reasoned.judge("before", subject.clone()).unwrap(),
            HookVerdict::Deny { reason } if reason == "no"
        ));
        // Anything else is an error — which the kernel reads as deny.
        let weird = RhaiGate::from_spool(&spec("42")).unwrap();
        assert!(weird.judge("before", subject).is_err());
    }

    #[test]
    fn the_script_sees_subject_and_chain() {
        let gate = RhaiGate::from_spool(&spec("chain == \"before\" && subject.name == \"stroke\""))
            .unwrap();
        assert_eq!(
            gate.judge("before", serde_json::json!({"name": "stroke"}))
                .unwrap(),
            HookVerdict::Allow
        );
        assert!(matches!(
            gate.judge("after", serde_json::json!({"name": "stroke"}))
                .unwrap(),
            HookVerdict::Deny { .. }
        ));
    }
}
