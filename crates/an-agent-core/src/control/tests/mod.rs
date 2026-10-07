use std::sync::{Arc, Mutex};

use super::*;
use crate::act::{Permit, Tool, ToolCtx, ToolTag};
use crate::agent::SessionManifest;
use crate::principal::card::{ModelSpec, ToolGrant, Topology};
use crate::testkit::{TempDir, bash_registry};
use crate::workspace::WorkspaceRecord;
use async_trait::async_trait;
use serde_json::Value;

fn card(id: &str, name: &str, permit: Permit) -> AgentCard {
    AgentCard {
        v: 1,
        id: Uuid::parse_str(id).unwrap(),
        model: ModelSpec {
            base_url: "https://example.com".into(),
            model: "m".into(),
            extra_body: None,
        },
        prompt: "p".into(),
        tools: vec![ToolGrant {
            name: name.into(),
            tag: ToolTag::none_permit(permit),
        }],
        topology: Topology::Leaf,
        kernel: "0.1.0".into(),
        supersedes: None,
    }
}

struct Gate {
    started: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}

#[async_trait]
impl Tool for Gate {
    fn name(&self) -> &str {
        "gate"
    }
    fn description(&self) -> &str {
        "holds the beat until released"
    }
    fn parameters(&self) -> Value {
        json!({})
    }
    async fn execute(&self, _args: Value, ctx: &ToolCtx) -> Result<String, String> {
        self.started.notify_one();
        let abort = async {
            if let Some(mut rx) = ctx.signal.clone() {
                loop {
                    if *rx.borrow() {
                        return;
                    }
                    if rx.changed().await.is_err() {
                        std::future::pending::<()>().await;
                    }
                }
            } else {
                std::future::pending::<()>().await;
            }
        };
        tokio::select! {
            _ = abort => Ok("cancelled".into()),
            _ = self.release.notified() => Ok("ran".into()),
        }
    }
}

fn shared_gate() -> &'static Gate {
    use std::sync::OnceLock;
    static GATE: OnceLock<Gate> = OnceLock::new();
    GATE.get_or_init(|| Gate {
        started: Arc::new(tokio::sync::Notify::new()),
        release: Arc::new(tokio::sync::Notify::new()),
    })
}

fn gate_ctor() -> Arc<dyn Tool> {
    Arc::new(SharedGate(shared_gate()))
}

struct SharedGate(&'static Gate);

#[async_trait]
impl Tool for SharedGate {
    fn name(&self) -> &str {
        self.0.name()
    }
    fn description(&self) -> &str {
        self.0.description()
    }
    fn parameters(&self) -> Value {
        self.0.parameters()
    }
    async fn execute(&self, args: Value, ctx: &ToolCtx) -> Result<String, String> {
        self.0.execute(args, ctx).await
    }
}

struct Clip;

impl SpoolBeat for Clip {
    fn receive(&self, event: &WorkspaceRecord) -> Result<String, String> {
        let body = event.body.clone().unwrap_or_default();
        if !body.contains("600,40") {
            return Err("not a stroke".into());
        }
        Ok(body
            .replace("600,40", "511,40")
            .replace("480,700", "480,511"))
    }
}

struct Ink {
    seen: Mutex<Option<String>>,
}

impl SpoolBeat for Ink {
    fn receive(&self, event: &WorkspaceRecord) -> Result<String, String> {
        *self.seen.lock().unwrap_or_else(|err| err.into_inner()) = event.body.clone();
        Ok("painted".into())
    }
}

struct Boom;

impl SpoolBeat for Boom {
    fn receive(&self, _event: &WorkspaceRecord) -> Result<String, String> {
        Err("crashed".into())
    }
}

fn stroke_card() -> AgentCard {
    card("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", "bash", Permit::Deny)
}

fn listed(events: &[(&str, &[&str])], config: &[&str], env: &[&str]) -> Registration {
    Registration {
        events: events
            .iter()
            .map(|(name, spools)| EventRoute {
                name: (*name).to_string(),
                spools: spools.iter().map(|spool| (*spool).to_string()).collect(),
            })
            .collect(),
        config: config.iter().map(|name| (*name).to_string()).collect(),
        env: env.iter().map(|name| (*name).to_string()).collect(),
    }
}

/// The mount-time claim: workspace events a body receives or may emit.
fn declares(consumes: &[&str], produces: &[&str]) -> SpoolDeclaration {
    SpoolDeclaration {
        consumes: consumes.iter().map(|name| (*name).to_string()).collect(),
        produces: produces.iter().map(|name| (*name).to_string()).collect(),
    }
}

mod hooks;
mod intents;
mod session;
mod spool;
mod thread;
mod workspace;
