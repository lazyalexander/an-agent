//! Subprocess tool adapter: run a script (bun TS today) as a `Tool` —
//! JSON args on stdin, one JSON result on stdout, process exits.
//! Process management mirrors the bash builtin: piped stdio, timeout,
//! output cap, kill on drop. Probe-grade.

use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use an_agent::act::{Tool, ToolCtx, ToolTag};
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

const CAP: usize = 64 * 1024;

pub struct SubprocessTool {
    name: String,
    description: String,
    params: Value,
    program: String,
    args: Vec<String>,
    tag: ToolTag,
    timeout: Duration,
}

impl SubprocessTool {
    pub fn bun_script(
        name: &str,
        description: &str,
        params: Value,
        script: impl Into<String>,
        tag: ToolTag,
    ) -> Arc<Self> {
        Arc::new(Self {
            name: name.into(),
            description: description.into(),
            params,
            program: "bun".into(),
            args: vec![script.into()],
            tag,
            timeout: Duration::from_secs(30),
        })
    }
}

#[async_trait::async_trait]
impl Tool for SubprocessTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters(&self) -> Value {
        self.params.clone()
    }

    fn tag_seed(&self) -> Option<ToolTag> {
        Some(self.tag.clone())
    }

    async fn execute(&self, args: Value, _ctx: &ToolCtx) -> Result<String, String> {
        let mut cmd = Command::new(&self.program);
        cmd.args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd.spawn().map_err(|e| e.to_string())?;
        let mut stdin = child.stdin.take().ok_or("stdin not piped")?;
        stdin
            .write_all(args.to_string().as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        drop(stdin); // close stdin so the script sees EOF
        match tokio::time::timeout(self.timeout, child.wait_with_output()).await {
            Err(_) => Err(format!("timed out after {}s", self.timeout.as_secs())),
            Ok(Err(e)) => Err(e.to_string()),
            Ok(Ok(out)) => {
                if !out.status.success() {
                    return Err(format!(
                        "exit {}: {}",
                        out.status.code().unwrap_or(-1),
                        String::from_utf8_lossy(&out.stderr)
                    ));
                }
                let s = String::from_utf8_lossy(&out.stdout);
                let capped = if s.len() > CAP { &s[..CAP] } else { &s };
                Ok(capped.trim().to_string())
            }
        }
    }
}
