//! The host's hook runner. Spool handlers are rhai gates from the
//! shelf, recovered by their pinned name and version, compiled once and
//! cached — a hook fires per delivery, so construction (the fail-fast
//! compile) must not. MCP handlers fail closed until the factory grows
//! MCP: an unimplemented mechanism is a deny, loud on tape, never a
//! silent skip.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::json;

use an_agent_core::control::{HookHandler, HookRunner, HookVerdict, SpoolReply};
use an_agent_core::workspace::WorkspaceRecord;
use an_agent_spool::gate::RhaiGate;
use an_agent_spool::spool::Registry;

pub struct HostHooks {
    registry: Registry,
    gates: Mutex<HashMap<(String, String), Arc<RhaiGate>>>,
}

impl HostHooks {
    pub fn new(registry: Registry) -> Self {
        Self {
            registry,
            gates: Mutex::new(HashMap::new()),
        }
    }

    fn gate(&self, name: &str, version: &str) -> Result<Arc<RhaiGate>, String> {
        let key = (name.to_string(), version.to_string());
        if let Some(gate) = self
            .gates
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .get(&key)
        {
            return Ok(Arc::clone(gate));
        }
        let spec = self
            .registry
            .recover(name, version)
            .map_err(|e| e.to_string())?;
        let gate = Arc::new(RhaiGate::from_spool(&spec)?);
        self.gates
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert(key, Arc::clone(&gate));
        Ok(gate)
    }
}

impl HookRunner for HostHooks {
    fn run_before(
        &self,
        handler: &HookHandler,
        event: &WorkspaceRecord,
    ) -> Result<HookVerdict, String> {
        match handler {
            HookHandler::Spool { name, version } => self.gate(name, version)?.judge(
                "before",
                json!({
                    "id": event.id,
                    "name": event.name,
                    "body": event.body,
                }),
            ),
            HookHandler::Mcp { .. } => Err("mcp hook handlers are not implemented yet".into()),
        }
    }

    fn run_after(&self, handler: &HookHandler, reply: &SpoolReply) -> Result<HookVerdict, String> {
        match handler {
            HookHandler::Spool { name, version } => self.gate(name, version)?.judge(
                "after",
                json!({
                    "spool": reply.spool,
                    "event_id": reply.event_id,
                    "reply": reply.reply,
                    "intents": reply.intents,
                }),
            ),
            HookHandler::Mcp { .. } => Err("mcp hook handlers are not implemented yet".into()),
        }
    }
}
