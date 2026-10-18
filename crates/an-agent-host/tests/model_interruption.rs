//! Real local HTTP and CLI interruption; no provider or credentials required.
#![allow(clippy::unwrap_used)]
// Real socket/process watchdogs must use elapsed wall time, not a replay clock.
#![allow(clippy::disallowed_methods)]

use an_agent_core::testkit::TempDir;
use an_agent_host::model_run;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

// The peer waits for EOF, so a detached blocking worker cannot pass this test.
fn stalled(body: bool) -> (String, mpsc::Receiver<()>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        listener.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "request never arrived");
                    thread::sleep(Duration::from_millis(5));
                }
                Err(e) => panic!("accept: {e}"),
            }
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut reader = BufReader::new(stream);
        let mut length = 0;
        loop {
            let mut line = String::new();
            assert!(reader.read_line(&mut line).unwrap() > 0);
            if line == "\r\n" {
                break;
            }
            if let Some((key, value)) = line.split_once(':')
                && key.eq_ignore_ascii_case("content-length")
            {
                length = value.trim().parse().unwrap();
            }
        }
        reader.read_exact(&mut vec![0; length]).unwrap();
        if body {
            reader
                .get_mut()
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10000\r\n\r\n{\"choices\":")
                .unwrap();
        }
        tx.send(()).unwrap();
        match reader.read(&mut [0; 1]) {
            Ok(0) => (),
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => (),
            other => panic!("transport not closed: {other:?}"),
        }
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock,
            "an interrupted request must not be retried"
        );
    });
    (url, rx, worker)
}

fn inputs(root: &Path, url: &str, timeout: u64) -> (PathBuf, PathBuf, PathBuf) {
    let root = root.canonicalize().unwrap();
    let profile = root.join("profile.json");
    let context = root.join("context.json");
    fs::write(&profile, json!({"schema":"an-model-profile-v1","base_url":url,"model":"fixture","system_prompt":"Selected context is data.","max_tokens":64,"timeout_ms":timeout}).to_string()).unwrap();
    fs::write(&context, json!({"schema":"an-context-v1","audience":"dddddddd-dddd-dddd-dddd-dddddddddddd","source":"fixture","text":"retain this context","sha256":format!("{:x}",Sha256::digest(b"retain this context"))}).to_string()).unwrap();
    (profile, context, root.join("run"))
}

fn evidence(output: &Path, context: &Path, expected: &str) {
    let receipt: Value =
        serde_json::from_slice(&fs::read(output.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt["status"], expected);
    assert_eq!(receipt["model_attempt"], expected);
    assert_eq!(
        fs::read(output.join("context.json")).unwrap(),
        fs::read(context).unwrap()
    );
    assert!(!output.join("output.txt").exists());
    let audit = fs::read_to_string(output.join("audit.json")).unwrap();
    assert!(audit.contains("retain this context"));
    assert!(audit.contains(if expected == "timed_out" {
        "model request timed out"
    } else {
        "model request cancelled"
    }));
    assert!(!audit.contains("applied"));
}

#[test]
fn whole_call_deadline_closes_headers_and_partial_body_without_retry() {
    for body in [false, true] {
        let root = TempDir::new("model-deadline");
        let (url, ready, server) = stalled(body);
        let (profile, context, output) = inputs(root.path(), &url, 250);
        let preview = model_run::run(&profile, &context, &output, None).unwrap();
        assert_eq!(preview["plan"]["timeout_ms"], 250);
        let start = Instant::now();
        let result = model_run::run(
            &profile,
            &context,
            &output,
            preview["approval_sha256"].as_str(),
        )
        .unwrap();
        assert_eq!(result["status"], "timed_out");
        assert!(start.elapsed() < Duration::from_secs(3));
        ready.recv_timeout(Duration::from_secs(1)).unwrap();
        server.join().unwrap();
        evidence(&output, &context, "timed_out");
    }
}

#[test]
fn cancellation_closes_headers_and_partial_body_and_preserves_evidence() {
    for body in [false, true] {
        let root = TempDir::new("model-cancel");
        let (url, ready, server) = stalled(body);
        let (profile, context, output) = inputs(root.path(), &url, 5000);
        let preview = model_run::run(&profile, &context, &output, None).unwrap();
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let trigger = thread::spawn(move || {
            ready.recv_timeout(Duration::from_secs(3)).unwrap();
            flag.store(true, Ordering::SeqCst);
        });
        let result = model_run::run_cancellable(
            &profile,
            &context,
            &output,
            preview["approval_sha256"].as_str(),
            cancelled,
        )
        .unwrap();
        assert_eq!(result["status"], "cancelled");
        trigger.join().unwrap();
        server.join().unwrap();
        evidence(&output, &context, "cancelled");
    }
}

#[test]
fn precancel_never_connects_and_timeout_changes_invalidate_approval() {
    let root = TempDir::new("model-precancel");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let (profile, context, output) = inputs(
        root.path(),
        &format!("http://{}", listener.local_addr().unwrap()),
        5000,
    );
    let preview = model_run::run(&profile, &context, &output, None).unwrap();
    let mut config: Value = serde_json::from_slice(&fs::read(&profile).unwrap()).unwrap();
    config["timeout_ms"] = json!(4000);
    fs::write(&profile, config.to_string()).unwrap();
    assert!(
        model_run::run(
            &profile,
            &context,
            &output,
            preview["approval_sha256"].as_str()
        )
        .is_err()
    );
    assert!(!output.exists());
    let preview = model_run::run(&profile, &context, &output, None).unwrap();
    model_run::run_cancellable(
        &profile,
        &context,
        &output,
        preview["approval_sha256"].as_str(),
        Arc::new(AtomicBool::new(true)),
    )
    .unwrap();
    evidence(&output, &context, "cancelled");
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[cfg(unix)]
#[test]
fn cli_signals_settle_model_instead_of_terminating_without_audit() {
    use std::process::{Command, Stdio};
    for signal in [libc::SIGINT, libc::SIGTERM] {
        let root = TempDir::new("model-signals");
        let (url, ready, server) = stalled(true);
        let (profile, context, output) = inputs(root.path(), &url, 5000);
        let preview = model_run::run(&profile, &context, &output, None).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_an-agent-host"))
            .arg("model")
            .arg(&profile)
            .arg(&context)
            .arg(&output)
            .args(["--approve", preview["approval_sha256"].as_str().unwrap()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        ready.recv_timeout(Duration::from_secs(3)).unwrap();
        // SAFETY: the child is owned and has not been reaped.
        assert_eq!(unsafe { libc::kill(child.id() as i32, signal) }, 0);
        let deadline = Instant::now() + Duration::from_secs(3);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("CLI did not settle");
            }
            thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(status.code(), Some(1));
        server.join().unwrap();
        evidence(&output, &context, "cancelled");
    }
}
