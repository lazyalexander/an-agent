//! Probe scaffolding shared by integration tests: the sequential ReAct loop
//! (one policy, not a kernel contract) and the chat-completions wire/client.
//! Formerly src/agent.rs and src/model.rs — moved out of the kernel so
//! probe-grade code cannot be mistaken for, or depended on by, formal code.
//! Src must never depend on this; deletion and rewrite are expected.

// clippy.toml's allow-*-in-tests covers #[test] fns only; this support module
// is ordinary code inside test crates, so it carries its own allowance.
#![allow(clippy::unwrap_used, clippy::expect_used)]

pub mod lean;
pub mod mount;
pub mod rhai;
pub mod silk;
pub mod ts_tool;

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use an_agent_core::act::{ActEnvelope, ActKind, Permit, Tool, ToolCall, ToolTag, effect_from_tag};
use an_agent_core::memstream::{ActOnEvent, AppendEvent, FromKind, Kind, Memevent};

// --- wire types (OpenAI-style chat completions; vendor detail) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<WireToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub function: WireFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone)]
pub struct Assistant {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub usage: Option<Usage>,
}

/// Token accounting for one model call; None when the wire omits it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("store: {0}")]
    Store(#[from] an_agent_core::memstream::StoreError),
    #[error("tool: {0}")]
    Tool(#[from] an_agent_core::act::ToolError),
    #[error("model: {0}")]
    Model(String),
}

pub trait Model: Send + Sync {
    /// Self-reported spec, taped on every invoke intent (thin trace, T2-style).
    fn spec(&self) -> serde_json::Value {
        serde_json::Value::Null
    }
    fn complete(
        &self,
        messages: &[ChatMessage],
        tools: &[Arc<dyn Tool>],
    ) -> impl std::future::Future<Output = Result<Assistant, AgentError>> + Send;
}

// --- the sequential ReAct loop (one policy) ---

pub struct AgentState {
    pub messages: Vec<ChatMessage>,
}

fn sha256_hex(bytes: impl AsRef<[u8]>) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes.as_ref())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

mod model;
mod policy;
mod step;

// Each probe binary uses a different subset. The paths stay on `support`.
#[allow(unused_imports)]
pub use model::{ChatCompletions, ModelSettings};
#[allow(unused_imports)]
pub use policy::{StepOpts, mount_policy, policy_step, run_policy_until_idle};
#[allow(unused_imports)]
pub use step::{last_assistant_text, run_until_idle, step};

pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "an-agent-test-{tag}-{}",
            an_agent_core::det_seam::Entropy::os()
                .ulid(an_agent_core::det_seam::Clock::wall().now_ms())
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        TempDir(dir)
    }

    pub fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
