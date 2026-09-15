use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use tokio::process::Command;
use tokio::time::timeout;

use crate::act::{Tool, ToolCtx, ToolTag};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_CAP: usize = 64 * 1024;
const KILL_GRACE: Duration = Duration::from_secs(1);

pub struct Bash {
    timeout: Duration,
    cap: usize,
}

impl Default for Bash {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_TIMEOUT,
            cap: DEFAULT_CAP,
        }
    }
}

impl Bash {
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            timeout,
            cap: DEFAULT_CAP,
        }
    }
}

fn cap_bytes(buf: &[u8], cap: usize) -> (String, bool) {
    if buf.len() <= cap {
        (String::from_utf8_lossy(buf).into_owned(), false)
    } else {
        (String::from_utf8_lossy(&buf[..cap]).into_owned(), true)
    }
}

#[cfg(unix)]
fn kill_group(pid: u32, sig: i32) {
    unsafe {
        libc::kill(-(pid as i32), sig);
    }
}

#[async_trait]
impl Tool for Bash {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Run a shell command. Arguments: { command: string }."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": { "command": { "type": "string" } },
            "required": ["command"]
        })
    }

    fn tag_seed(&self) -> Option<ToolTag> {
        Some(ToolTag::unbounded())
    }

    async fn execute(&self, args: Value, ctx: &ToolCtx) -> Result<String, String> {
        if ctx
            .signal
            .as_ref()
            .is_some_and(|s| *s.borrow())
        {
            return Ok("cancelled; process killed".into());
        }
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "command is required".to_string())?;
        if command.is_empty() {
            return Err("command is required".into());
        }

        let mut cmd = Command::new("bash");
        cmd.arg("-c")
            .arg(command)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        cmd.process_group(0);

        let child = cmd.spawn().map_err(|e| e.to_string())?;
        let pid = child.id();

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
                std::future::pending::<()>().await
            }
        };

        tokio::select! {
            _ = abort => {
                if let Some(pid) = pid {
                    #[cfg(unix)]
                    {
                        kill_group(pid, libc::SIGTERM);
                        tokio::time::sleep(KILL_GRACE).await;
                        kill_group(pid, libc::SIGKILL);
                    }
                }
                Ok("cancelled; process killed".into())
            }
            timed = timeout(self.timeout, child.wait_with_output()) => {
                match timed {
                    Ok(Ok(output)) => {
                        let (out_s, out_t) = cap_bytes(&output.stdout, self.cap);
                        let (err_s, err_t) = cap_bytes(&output.stderr, self.cap);
                        let mut parts = Vec::new();
                        if !output.status.success() {
                            parts.push(format!("exit {}", output.status.code().unwrap_or(-1)));
                        }
                        if !out_s.is_empty() {
                            parts.push(out_s);
                        }
                        if out_t {
                            parts.push("[stdout truncated]".into());
                        }
                        if !err_s.is_empty() {
                            parts.push(format!("[stderr]\n{err_s}"));
                        }
                        if err_t {
                            parts.push("[stderr truncated]".into());
                        }
                        Ok(parts.join("\n"))
                    }
                    Ok(Err(e)) => Err(e.to_string()),
                    Err(_) => {
                        if let Some(pid) = pid {
                            #[cfg(unix)]
                            {
                                kill_group(pid, libc::SIGTERM);
                                tokio::time::sleep(KILL_GRACE).await;
                                kill_group(pid, libc::SIGKILL);
                            }
                        }
                        Ok(format!(
                            "timed out after {}s; process killed",
                            self.timeout.as_secs()
                        ))
                    }
                }
            }
        }
    }
}

#[cfg(all(test, unix))]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use serde_json::json;
    use tokio::sync::watch;

    fn ctx_open() -> ToolCtx {
        let (_tx, rx) = watch::channel(false);
        ToolCtx { signal: Some(rx) }
    }

    #[tokio::test]
    async fn times_out_and_kills_background_child() {
        let dir = std::env::temp_dir().join(format!("an-agent-bash-{}", ulid::Ulid::new()));
        fs::create_dir_all(&dir).unwrap();
        let pid_file: PathBuf = dir.join("sleep.pid");
        let bash = Bash::with_timeout(Duration::from_millis(400));
        let cmd = format!(r#"sleep 30 & echo $! > "{}"; wait"#, pid_file.display());
        let out = bash
            .execute(json!({ "command": cmd }), &ctx_open())
            .await
            .unwrap();
        assert!(out.contains("timed out"), "{out}");
        let pid: i32 = fs::read_to_string(&pid_file).unwrap().trim().parse().unwrap();
        tokio::time::sleep(Duration::from_millis(150)).await;
        let still = unsafe { libc::kill(pid, 0) };
        assert_eq!(still, -1, "grandchild should be gone");
    }
}
