//! silk link probe, P0-1: envelopes and delivery. A policy spool yields a
//! silk tell as its continuation; the driver stamps and tapes it; a second
//! policy answers an ask by correlation. Asserts on the tape only:
//! stamping, refs interlock, single termination, and that a script naming
//! its own sender is refused.

// Helper fns here are not #[test] fns, so allow-*-in-tests does not reach them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[allow(dead_code)]
mod support;

use std::sync::{Arc, Mutex};

use an_agent_core::act::{ActCtx, FileFacet, MemoryFacet, Tool, ToolCtx, ToolTag};
use an_agent_core::memstream::{AppendEvent, FromKind, JsonlStore, Kind};
use an_agent_spool::descriptor::{Net, Proc};
use an_agent_spool::spool::{Faces, Flow, Registry};
use serde_json::{Map, Value, json};
use sha2::Digest;
use support::mount::{HostCtors, Mounter};
use support::silk::{Envelope, OutEnvelope, SilkKind};
use support::{AgentError, Assistant, Model, StepOpts, TempDir, policy_step};

const SESSION: &str = "s1";

struct NoModel;
impl Model for NoModel {
    async fn complete(
        &self,
        _messages: &[support::ChatMessage],
        _tools: &[Arc<dyn Tool>],
    ) -> Result<Assistant, AgentError> {
        Ok(Assistant {
            content: String::new(),
            tool_calls: vec![],
            usage: None,
        })
    }
}

fn ctx() -> ToolCtx {
    let (_tx, rx) = tokio::sync::watch::channel(false);
    ToolCtx { signal: Some(rx) }
}

fn rights() -> Faces {
    Faces {
        file: FileFacet::None,
        memory: MemoryFacet::Ignore,
        net: Net::None,
        proc_: Proc::None,
        flow: Flow::Both,
    }
}

fn silk_envelopes(store: &JsonlStore) -> Vec<Envelope> {
    store
        .read_all()
        .unwrap()
        .iter()
        .filter(|e| e.tags.iter().any(|t| t == "silk"))
        .filter_map(|e| serde_json::from_str(&e.content).ok())
        .collect()
}

#[path = "silk_link/link.rs"]
mod link;
#[path = "silk_link/screen.rs"]
mod screen;
#[path = "silk_link/tell.rs"]
mod tell;
#[path = "silk_link/world.rs"]
mod world;
