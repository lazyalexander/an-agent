//! The real model client against a stub wire: a one-shot HTTP server on
//! loopback captures the request and answers with a canned response, so
//! the client is tested through the actual HTTP path — request shape,
//! auth header, usage parsing, failure honesty.

#![allow(clippy::unwrap_used)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::mpsc::{Receiver, channel};

use serde_json::{Value, json};

use an_agent_core::control::{ModelClient, ModelMessage};
use an_agent_core::principal::card::ModelSpec;
use an_agent_host::HttpModelClient;

/// What the stub captured from one request.
struct Captured {
    path: String,
    auth: Option<String>,
    body: Value,
}

/// Serve exactly one request: capture it, answer with `status` and
/// `body`, hand the capture back over the channel.
fn serve_once(status: u16, body: &str) -> (String, Receiver<Captured>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = channel();
    let body = body.to_string();
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream);
        let mut request_line = String::new();
        reader.read_line(&mut request_line).unwrap();
        let path = request_line
            .split_whitespace()
            .nth(1)
            .unwrap_or_default()
            .to_string();
        let mut auth = None;
        let mut content_length = 0usize;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let line = line.trim();
            if line.is_empty() {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                match name.trim().to_ascii_lowercase().as_str() {
                    "authorization" => auth = Some(value.trim().to_string()),
                    "content-length" => content_length = value.trim().parse().unwrap_or(0),
                    _ => {}
                }
            }
        }
        let mut raw = vec![0u8; content_length];
        reader.read_exact(&mut raw).unwrap();
        let captured = Captured {
            path,
            auth,
            body: serde_json::from_slice(&raw).unwrap_or(Value::Null),
        };
        let response = format!(
            "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        reader.get_mut().write_all(response.as_bytes()).unwrap();
        tx.send(captured).unwrap();
    });
    (url, rx)
}

fn spec(base_url: &str) -> ModelSpec {
    ModelSpec {
        base_url: base_url.into(),
        model: "m".into(),
        extra_body: Some(json!({ "temperature": 0.2 })),
    }
}

fn messages() -> Vec<ModelMessage> {
    vec![
        ModelMessage {
            role: "system".into(),
            content: "you keep the tavern".into(),
        },
        ModelMessage {
            role: "user".into(),
            content: "[scene] a traveler enters\n".into(),
        },
    ]
}

const OK_BODY: &str = r#"{"choices":[{"message":{"role":"assistant","content":"welcome, traveler"}}],"usage":{"prompt_tokens":42,"completion_tokens":7}}"#;

#[test]
fn posts_the_chat_completion_and_reads_usage() {
    let (url, rx) = serve_once(200, OK_BODY);
    let client = HttpModelClient::bearer("sk-test").unwrap();
    let completion = client.complete(&spec(&url), messages()).unwrap();

    assert_eq!(completion.content, "welcome, traveler");
    let usage = completion.usage.expect("usage");
    assert_eq!(usage.input_tokens, 42);
    assert_eq!(usage.output_tokens, 7);

    let captured = rx.recv().unwrap();
    assert_eq!(captured.path, "/chat/completions");
    assert_eq!(captured.auth.as_deref(), Some("Bearer sk-test"));
    assert_eq!(captured.body["model"], "m");
    assert_eq!(captured.body["messages"][0]["role"], "system");
    assert_eq!(
        captured.body["messages"][1]["content"],
        "[scene] a traveler enters\n"
    );
    // The provider knob joined the body.
    assert_eq!(captured.body["temperature"], 0.2);
}

#[test]
fn extra_body_cannot_shadow_identity() {
    let (url, rx) = serve_once(200, OK_BODY);
    let client = HttpModelClient::new().unwrap();
    let mut spec = spec(&url);
    spec.extra_body = Some(json!({ "model": "evil", "messages": [] }));
    client.complete(&spec, messages()).unwrap();

    let captured = rx.recv().unwrap();
    assert_eq!(captured.body["model"], "m");
    assert_eq!(captured.body["messages"].as_array().unwrap().len(), 2);
    assert!(captured.auth.is_none());
}

#[test]
fn a_response_without_usage_is_an_error() {
    let body = r#"{"choices":[{"message":{"role":"assistant","content":"hi"}}]}"#;
    let (url, _rx) = serve_once(200, body);
    let client = HttpModelClient::new().unwrap();
    let err = client.complete(&spec(&url), messages()).unwrap_err();
    assert!(err.contains("usage"), "{err}");
}

#[test]
fn an_http_failure_carries_status_and_a_bounded_snippet() {
    let body = "x".repeat(4_000);
    let (url, _rx) = serve_once(500, &body);
    let client = HttpModelClient::new().unwrap();
    let err = client.complete(&spec(&url), messages()).unwrap_err();
    assert!(err.contains("500"), "{err}");
    assert!(err.len() < 4_000);
}

#[test]
fn a_response_without_content_is_an_error() {
    let body = r#"{"choices":[],"usage":{"prompt_tokens":1,"completion_tokens":1}}"#;
    let (url, _rx) = serve_once(200, body);
    let client = HttpModelClient::new().unwrap();
    let err = client.complete(&spec(&url), messages()).unwrap_err();
    assert!(err.contains("content"), "{err}");
}
