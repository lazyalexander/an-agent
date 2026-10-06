//! The rhai constructor, beat sort. A published spool body receives one
//! workspace event and returns its reply text — the event-receiver half
//! of a channel or character body. Same sandbox as the policy sort:
//! bounded engine, empty file/net/proc faces enforced at construction;
//! anything effectful leaves as the reply and the host disposes.
//!
//! The script sees `event` (`#{ id, name, body }`, all strings) and the
//! mount `config`, and must return a string — the reply the core tapes
//! and hands back to the host. The script is compiled at construction:
//! a syntax error fails the mount, never the first event.

use serde_json::{Map, Value};

use an_agent_core::control::SpoolBeat;
use an_agent_core::workspace::WorkspaceRecord;

use crate::descriptor::{Net, Proc};
use crate::spool::{Constructor, SpoolSpec};

pub struct RhaiBeat {
    engine: rhai::Engine,
    ast: rhai::AST,
    config: rhai::Dynamic,
}

impl RhaiBeat {
    /// Build a beat from a published body. Construction is the fail-fast
    /// gate: the constructor must be rhai, the effector faces must be
    /// empty, and the script must compile.
    pub fn from_spool(spec: &SpoolSpec, config: Map<String, Value>) -> Result<Self, String> {
        use an_agent_core::act::FileFacet;
        if spec.effect.file != FileFacet::None {
            return Err(format!(
                "beat declares a file face it cannot use: {:?}",
                spec.effect.file
            ));
        }
        if spec.effect.net != Net::None {
            return Err("beat declares net it cannot use".into());
        }
        if spec.effect.proc_ != Proc::None {
            return Err("beat declares proc it cannot use".into());
        }
        let script = match &spec.constructor {
            Constructor::Rhai { script } => script,
            _ => return Err("not a rhai spool".into()),
        };
        let engine = crate::policy::bounded_engine();
        let ast = engine.compile(script).map_err(|e| e.to_string())?;
        Ok(Self {
            engine,
            ast,
            config: rhai::serde::to_dynamic(config).map_err(|e| e.to_string())?,
        })
    }
}

impl SpoolBeat for RhaiBeat {
    fn receive(&self, event: &WorkspaceRecord) -> Result<String, String> {
        let mut scope = rhai::Scope::new();
        scope.push(
            "event",
            rhai::serde::to_dynamic(serde_json::json!({
                "id": event.id,
                "name": event.name,
                "body": event.body,
            }))
            .map_err(|e| e.to_string())?,
        );
        scope.push_dynamic("config", self.config.clone());
        let out = self
            .engine
            .eval_ast_with_scope::<rhai::Dynamic>(&mut scope, &self.ast)
            .map_err(|e| e.to_string())?;
        out.into_string()
            .map_err(|e| format!("beat reply must be a string: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spool;

    fn spec(script: &str) -> SpoolSpec {
        let yaml = format!(
            "v: 1\nkind: spool\nname: b\nversion: 1.0.0\nsummary: b\nconstructor: rhai\nscript: |\n  {script}\neffect:\n  net: none\n  file: {{ op: none }}\n  proc: none\n  memory: {{ op: ignore }}\n  flow: none\ninverse: none\nrequires: []\nconsumes: []\nproduces: []\n"
        );
        spool::parse(&yaml).unwrap()
    }

    fn event(body: &str) -> WorkspaceRecord {
        WorkspaceRecord {
            id: "e1".into(),
            kind: "event".into(),
            body: Some(body.into()),
            sha256: None,
            at: None,
            name: Some("discord.message".into()),
        }
    }

    #[test]
    fn construction_is_the_fail_fast_gate() {
        // A syntax error fails at construction, not at the first event.
        assert!(RhaiBeat::from_spool(&spec("let x = ;"), Map::new()).is_err());
        // A non-rhai constructor is refused.
        let host = spool::parse(
            "v: 1\nkind: spool\nname: h\nversion: 1.0.0\nsummary: h\nconstructor: host\nhost: echo\neffect:\n  net: none\n  file: { op: none }\n  proc: none\n  memory: { op: ignore }\n  flow: none\ninverse: none\nrequires: []\nconsumes: []\nproduces: []\n",
        )
        .unwrap();
        assert!(RhaiBeat::from_spool(&host, Map::new()).is_err());
        // An effector face is refused.
        let mut yaml_spec = spec("42");
        yaml_spec.effect.file = an_agent_core::act::FileFacet::Unbounded;
        assert!(RhaiBeat::from_spool(&yaml_spec, Map::new()).is_err());
    }

    #[test]
    fn the_reply_must_be_a_string() {
        let beat = RhaiBeat::from_spool(&spec("42"), Map::new()).unwrap();
        assert!(
            SpoolBeat::receive(&beat, &event("x"))
                .unwrap_err()
                .contains("must be a string")
        );
        let ok = RhaiBeat::from_spool(&spec("\"yo\""), Map::new()).unwrap();
        assert_eq!(SpoolBeat::receive(&ok, &event("x")).unwrap(), "yo");
    }
}
