//! Hook chains on the delivery path: `before` runs between the event and
//! its spools, `after` between a reply and the host. Two fixed points,
//! not open-ended names. v1 is the gate gear only — allow or deny; the
//! transform gear (rewrite, both versions taped) is a later slice.
//!
//! Core does not know MCP or the spool shelf. It hands the declared
//! handler and the subject to the host's [`HookRunner`] and takes a
//! verdict. Chain order and parameters live in config (data); the
//! admission logic lives here (code).
//!
//! Taping discipline: a denial is always taped; an allow never is —
//! allows are the default path, and at event frequency they would flood
//! the tape (the same discipline as the fast lane). A hook that fails
//! is a deny: fail-closed.

use std::sync::Arc;

use serde_json::json;

use super::workspace::HookHandler;
use super::{AgentControl, ControlError, SpoolReply};
use crate::agent::Agent;
use crate::workspace::WorkspaceRecord;

/// One verdict from one hook. Gate gear only in v1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookVerdict {
    Allow,
    Deny { reason: String },
}

/// The host runs hooks. A spool handler is built from the shelf, an MCP
/// handler goes through the factory — both are the host's business.
pub trait HookRunner: Send + Sync {
    fn run_before(
        &self,
        handler: &HookHandler,
        event: &WorkspaceRecord,
    ) -> Result<HookVerdict, String>;
    fn run_after(&self, handler: &HookHandler, reply: &SpoolReply) -> Result<HookVerdict, String>;
}

impl AgentControl {
    /// Install the host's hook runner. Declared hooks with no runner make
    /// delivery fail closed (`HookRunnerMissing`), never silently skip.
    pub fn set_hook_runner(&self, runner: Arc<dyn HookRunner>) {
        let mut slot = self
            .inner
            .hook_runner
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        *slot = Some(runner);
    }

    /// The before chain: every handler in config order, first deny wins.
    pub(super) fn hooks_before(
        &self,
        agent: &Agent,
        event: &WorkspaceRecord,
    ) -> Result<(), ControlError> {
        let Some(config) = self.current_config()? else {
            return Ok(());
        };
        if config.hooks.before.is_empty() {
            return Ok(());
        }
        let runner = self.hook_runner()?;
        for handler in &config.hooks.before {
            let verdict =
                runner
                    .run_before(handler, event)
                    .unwrap_or_else(|err| HookVerdict::Deny {
                        reason: format!("hook failed: {err}"),
                    });
            if let HookVerdict::Deny { reason } = verdict {
                self.note_hook(agent, "before", handler, &reason)?;
                return Err(ControlError::HookDenied {
                    chain: "before".into(),
                    reason,
                });
            }
        }
        Ok(())
    }

    /// The after chain on one reply. A deny does not reach the host; the
    /// spool note is already on tape, and the denial joins it.
    pub(super) fn hooks_after(
        &self,
        agent: &Agent,
        reply: &SpoolReply,
    ) -> Result<(), ControlError> {
        let Some(config) = self.current_config()? else {
            return Ok(());
        };
        if config.hooks.after.is_empty() {
            return Ok(());
        }
        let runner = self.hook_runner()?;
        for handler in &config.hooks.after {
            let verdict =
                runner
                    .run_after(handler, reply)
                    .unwrap_or_else(|err| HookVerdict::Deny {
                        reason: format!("hook failed: {err}"),
                    });
            if let HookVerdict::Deny { reason } = verdict {
                self.note_hook(agent, "after", handler, &reason)?;
                return Err(ControlError::HookDenied {
                    chain: "after".into(),
                    reason,
                });
            }
        }
        Ok(())
    }

    fn hook_runner(&self) -> Result<Arc<dyn HookRunner>, ControlError> {
        self.inner
            .hook_runner
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
            .ok_or(ControlError::HookRunnerMissing)
    }

    fn note_hook(
        &self,
        agent: &Agent,
        chain: &str,
        handler: &HookHandler,
        reason: &str,
    ) -> Result<String, ControlError> {
        let content = json!({
            "chain": chain,
            "handler": serde_json::to_value(handler)
                .unwrap_or_else(|_| json!("unrepresentable")),
            "verdict": "deny",
            "reason": reason,
        });
        self.append(agent, "hook", content.to_string())
    }
}
