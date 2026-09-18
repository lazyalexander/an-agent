//! Lean 4 CLI as a `Tool`. Probe-grade: spawn `lean` on a snippet or
//! `lake build` in a copied Lake package. Timeout + byte cap; no in-process
//! Lean runtime.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use an_agent::act::{Tool, ToolCtx, ToolTag};
use serde_json::{Value, json};
use tokio::process::Command;
use tokio::time::timeout;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
const DEFAULT_CAP: usize = 64 * 1024;

pub struct LeanTool {
    lean: PathBuf,
    lake: PathBuf,
    timeout: Duration,
    cap: usize,
}

impl LeanTool {
    /// Resolves `lean`/`lake` from `ELAN_HOME` / `~/.elan/bin`, then PATH.
    pub fn discover() -> Result<Arc<Self>, String> {
        Ok(Arc::new(Self {
            lean: resolve("lean").ok_or("lean not found (install elan + stable)")?,
            lake: resolve("lake").ok_or("lake not found (install elan + stable)")?,
            timeout: DEFAULT_TIMEOUT,
            cap: DEFAULT_CAP,
        }))
    }

    pub fn with_timeout(self: &Arc<Self>, timeout: Duration) -> Arc<Self> {
        Arc::new(Self {
            lean: self.lean.clone(),
            lake: self.lake.clone(),
            timeout,
            cap: self.cap,
        })
    }
}

fn elan_bin() -> Option<PathBuf> {
    if let Some(h) = std::env::var_os("ELAN_HOME") {
        return Some(PathBuf::from(h).join("bin"));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".elan/bin"))
}

fn resolve(name: &str) -> Option<PathBuf> {
    if let Some(dir) = elan_bin() {
        let p = dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    let ok = std::process::Command::new(name)
        .arg("--version")
        .output()
        .ok()?
        .status
        .success();
    ok.then(|| PathBuf::from(name))
}

fn cap_bytes(buf: &[u8], cap: usize) -> (String, bool) {
    if buf.len() <= cap {
        (String::from_utf8_lossy(buf).into_owned(), false)
    } else {
        (String::from_utf8_lossy(&buf[..cap]).into_owned(), true)
    }
}

fn payload(ok: bool, exit: i32, stdout: &[u8], stderr: &[u8], cap: usize) -> String {
    let (out, out_t) = cap_bytes(stdout, cap);
    let (err, err_t) = cap_bytes(stderr, cap);
    json!({
        "ok": ok,
        "exit": exit,
        "stdout": out,
        "stderr": err,
        "truncated": out_t || err_t,
    })
    .to_string()
}

async fn run_cmd(
    bin: &Path,
    args: &[&str],
    cwd: &Path,
    timeout_d: Duration,
    cap: usize,
) -> Result<String, String> {
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = cmd.spawn().map_err(|e| e.to_string())?;
    match timeout(timeout_d, child.wait_with_output()).await {
        Err(_) => Ok(json!({
            "ok": false,
            "exit": -1,
            "stdout": "",
            "stderr": format!("timed out after {}s", timeout_d.as_secs()),
            "truncated": false,
        })
        .to_string()),
        Ok(Err(e)) => Err(e.to_string()),
        Ok(Ok(out)) => {
            let code = out.status.code().unwrap_or(-1);
            Ok(payload(
                out.status.success(),
                code,
                &out.stdout,
                &out.stderr,
                cap,
            ))
        }
    }
}

#[async_trait::async_trait]
impl Tool for LeanTool {
    fn name(&self) -> &str {
        "lean_check"
    }

    fn description(&self) -> &str {
        "Check Lean 4 source (`source`) or `lake build` a project (`project`)."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "source": { "type": "string" },
                "project": { "type": "string" }
            }
        })
    }

    fn tag_seed(&self) -> Option<ToolTag> {
        Some(ToolTag::unbounded())
    }

    async fn execute(&self, args: Value, ctx: &ToolCtx) -> Result<String, String> {
        if ctx.signal.as_ref().is_some_and(|s| *s.borrow()) {
            return Ok(json!({
                "ok": false,
                "exit": -1,
                "stdout": "",
                "stderr": "cancelled",
                "truncated": false,
            })
            .to_string());
        }
        if let Some(source) = args.get("source").and_then(|v| v.as_str()) {
            let dir = std::env::temp_dir().join(format!(
                "an-agent-lean-{}",
                an_agent::det_seam::Entropy::os().ulid(an_agent::det_seam::Clock::wall().now_ms())
            ));
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let file = dir.join("Main.lean");
            std::fs::write(&file, source).map_err(|e| e.to_string())?;
            let result = run_cmd(
                &self.lean,
                &[file.to_str().ok_or("path not utf-8")?],
                &dir,
                self.timeout,
                self.cap,
            )
            .await;
            let _ = std::fs::remove_dir_all(&dir);
            return result;
        }
        if let Some(project) = args.get("project").and_then(|v| v.as_str()) {
            let root = PathBuf::from(project);
            if !root.join("lakefile.toml").is_file() && !root.join("lakefile.lean").is_file() {
                return Err("project is not a Lake package".into());
            }
            return run_cmd(&self.lake, &["build"], &root, self.timeout, self.cap).await;
        }
        Err("source or project is required".into())
    }
}
