//! Shared test scaffolding: self-cleaning temporary directories and a
//! stand-in tool constructor. The kernel registers no real tools.
//! Compiled into the library only behind the `testkit` feature so other
//! crates' tests can use it; the expects here are the test-module allowance.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::act::{Tool, ToolCtx};
use crate::principal::factory::ToolCtor;

/// Constructor list whose only entry is named `bash` and does not run a shell.
pub fn bash_registry() -> Vec<(&'static str, ToolCtor)> {
    vec![("bash", || Arc::new(BashStub))]
}

struct BashStub;

#[async_trait::async_trait]
impl Tool for BashStub {
    fn name(&self) -> &str {
        "bash"
    }
    fn description(&self) -> &str {
        "stub"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({})
    }
    async fn execute(&self, _args: serde_json::Value, _ctx: &ToolCtx) -> Result<String, String> {
        Ok("ran".into())
    }
}

/// A temporary directory that deletes itself on drop.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> Self {
        let id = crate::det_seam::Entropy::os().ulid(crate::det_seam::Clock::wall().now_ms());
        let dir = std::env::temp_dir().join(format!("an-agent-{tag}-{id}"));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        Self(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
