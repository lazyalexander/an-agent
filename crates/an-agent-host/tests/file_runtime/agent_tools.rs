//! Exercise the real MCP CLI with a scripted client; no paid model needed in CI.
#![allow(clippy::disallowed_methods)]
use super::*;
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

struct Client {
    child: Child,
    input: Option<ChildStdin>,
    output: mpsc::Receiver<Value>,
}
impl Client {
    fn start(profile: &Path, approval: &str) -> execution::Result<Self> {
        let mut child = Command::new(env!("CARGO_BIN_EXE_an-agent-tools"))
            .args([profile.to_str().ok_or("path")?, "--approve", approval])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        let input = child.stdin.take();
        let stdout = child.stdout.take().ok_or("stdout")?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let Ok(v) = serde_json::from_str(&line) else {
                    break;
                };
                if tx.send(v).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            input,
            output: rx,
        })
    }
    fn send(&mut self, value: Value) -> execution::Result<()> {
        let input = self.input.as_mut().ok_or("closed client")?;
        writeln!(input, "{value}")?;
        input.flush()?;
        Ok(())
    }
    fn response(&self, id: u64) -> execution::Result<Value> {
        for _ in 0..16 {
            let v = self.output.recv_timeout(Duration::from_secs(10))?;
            if v["id"] == id {
                return Ok(v);
            }
        }
        Err("missing matching MCP response".into())
    }
    fn call(&mut self, id: u64, name: &str, args: Value) -> execution::Result<Value> {
        self.send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":args}}))?;
        let v = self.response(id)?;
        Ok(serde_json::from_str(
            v["result"]["content"][0]["text"]
                .as_str()
                .ok_or("missing result content")?,
        )?)
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        self.input.take();
        let start = Instant::now();
        loop {
            if self.child.try_wait().ok().flatten().is_some() {
                break;
            }
            if start.elapsed() > Duration::from_secs(8) {
                let _ = self.child.kill();
                let _ = self.child.wait();
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn active_run(
    root: &Path,
    previous: &std::collections::BTreeSet<String>,
) -> execution::Result<PathBuf> {
    let start = Instant::now();
    loop {
        for entry in fs::read_dir(root.join("store/runs"))? {
            let p = entry?.path();
            let name = p
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or("invalid run name")?;
            if previous.contains(name) {
                continue;
            }
            if fs::read(p.join("work/output.txt")).ok().as_deref() == Some(b"partial") {
                return Ok(p);
            }
        }
        if start.elapsed() > Duration::from_secs(10) {
            return Err("kit never became active".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn settled(run: &Path, published: &Path) -> execution::Result<Value> {
    let start = Instant::now();
    loop {
        // Publication is the completion fence. The native receipt is written
        // before it; observing that file's creation alone can see an open write.
        if published.exists() {
            let result: Value = serde_json::from_slice(&fs::read(published)?)?;
            assert_eq!(result["status"], "cancelled");
            return Ok(serde_json::from_slice(&fs::read(
                run.join("receipt.json"),
            )?)?);
        }
        if start.elapsed() > Duration::from_secs(8) {
            return Err("missing termination receipt".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn mcp_permissions_artifact_handoff_cancellation_and_disconnect() -> execution::Result<()> {
    let f = Fixture::new()?;
    let profile = f.root.join("grant.json");
    let session = f.root.join("tool-session");
    fs::write(
        &profile,
        serde_json::to_vec(
            &json!({"schema":"an-agent-tools-v1","store":f.root.join("store"),"session":session,"source":f.root.join("main.rs"),"kits":{"sort":f.release},"max_calls":12}),
        )?,
    )?;
    let preview = an_agent_host::agent_tools::preview(&profile)?;
    let mut client = Client::start(&profile, preview["approval_sha256"].as_str().unwrap())?;
    client.send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"fixture","version":"1"}}}))?;
    assert!(client.response(1)?["result"]["capabilities"]["tools"].is_object());
    client.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))?;
    client.send(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}))?;
    assert_eq!(
        client.response(2)?["result"]["tools"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let source = client.call(20, "read_source", json!({"invocation":"read-1"}))?;
    assert_eq!(
        source["source"],
        fs::read_to_string(f.root.join("main.rs"))?
    );
    let proposed = client.call(
        21,
        "propose_source",
        json!({"invocation":"proposal-1","text":"candidate only"}),
    )?;
    assert_eq!(proposed["accepted"], false);
    assert_eq!(proposed["executed"], false);
    assert_eq!(
        fs::read(session.join("calls/proposal-1/candidate.rs"))?,
        b"candidate only"
    );
    assert!(!f.root.join("store/runs").exists());
    let outside = f.root.join("outside.rs");
    let refused = client.call(
        3,
        "propose_source",
        json!({"invocation":"bad-path","text":"bad","path":outside}),
    )?;
    assert_eq!(refused["status"], "refused-or-uncertain");
    assert!(!outside.exists());
    assert_eq!(fs::read_dir(session.join("denials"))?.count(), 1);
    let refused = client.call(
        4,
        "run_kit",
        json!({"invocation":"not-granted","kit":"shell","text":"true"}),
    )?;
    assert_eq!(refused["status"], "refused-or-uncertain");
    assert!(!f.root.join("store/runs").exists());
    let result = client.call(
        5,
        "run_kit",
        json!({"invocation":"sort-1","kit":"sort","text":"alpha\n"}),
    )?;
    assert_eq!(result["status"], "succeeded");
    assert_eq!(result["text"], "ALPHA\n");
    let revision = result["artifact"]["revision"].as_str().unwrap();
    an_agent_host::artifact_versions::checkout(
        &session.join("artifacts"),
        revision,
        &f.root.join("consumed.txt"),
    )?;
    assert_eq!(fs::read(f.root.join("consumed.txt"))?, b"ALPHA\n");
    assert_eq!(
        client.call(
            6,
            "run_kit",
            json!({"invocation":"sort-1","kit":"sort","text":"alpha\n"})
        )?["status"],
        "refused-or-uncertain"
    );
    for (id, invocation, disconnect) in [(7, "cancel-1", false), (8, "disconnect-1", true)] {
        let before = fs::read_dir(f.root.join("store/runs"))?
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        client.send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"run_kit","arguments":{"invocation":invocation,"kit":"sort","text":"hang"}}}))?;
        let run = active_run(&f.root, &before)?;
        if disconnect {
            client.input.take();
        } else {
            client.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id,"reason":"operator cancelled"}}))?;
        }
        let receipt = settled(
            &run,
            &session.join("calls").join(invocation).join("result.json"),
        )?;
        assert_eq!(receipt["status"], "cancelled");
        assert_eq!(receipt["process"]["reaped"], true);
        assert!(!run.join("artifact.txt").exists());
    }
    assert_eq!(
        fs::read_dir(session.join("artifacts/revisions"))?.count(),
        1
    );
    Ok(())
}
