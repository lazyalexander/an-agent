//! The real model client: OpenAI-compatible chat completions over HTTP.
//! Sync and blocking, like the trait it implements — the async world, if
//! a host lives in one, wraps this in its own runtime.
//!
//! Two honesty rules, both fail-loud:
//!
//! - **Usage is required.** The thin tape accounts cost from the
//!   provider's usage report; a response without one is a protocol
//!   violation here, not a gap to paper over. Stub clients may still
//!   report `None`; the real wire may not.
//! - **Identity is not an escape hatch.** The card's `extra_body` adds
//!   provider knobs (temperature, penalties), but `model` and `messages`
//!   are written after it and cannot be shadowed by config.

use std::time::Duration;

use serde_json::{Map, Value, json};

use an_agent_core::control::{Completion, ModelClient, ModelMessage, Usage};
use an_agent_core::principal::card::ModelSpec;

/// Model calls are slow; a hung one is not forever. Connect fails fast,
/// the whole call gets five minutes.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const CALL_TIMEOUT: Duration = Duration::from_secs(300);

/// How much of an error response body reaches the error string.
const ERROR_SNIPPET: usize = 512;

/// A blocking HTTP [`ModelClient`] for OpenAI-compatible endpoints
/// (`POST {base_url}/chat/completions`). Bearer auth is the host's to
/// give — the key never lives on a card, so the tape never records it.
pub struct HttpModelClient {
    client: reqwest::blocking::Client,
    auth: Option<String>,
}

impl HttpModelClient {
    /// A client without credentials — local endpoints, or a proxy that
    /// authenticates upstream.
    pub fn new() -> Result<Self, String> {
        Self::build(None)
    }

    /// A client sending `Authorization: Bearer <key>` on every call.
    pub fn bearer(key: impl Into<String>) -> Result<Self, String> {
        Self::build(Some(key.into()))
    }

    fn build(auth: Option<String>) -> Result<Self, String> {
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(CALL_TIMEOUT)
            .build()
            .map_err(|e| format!("model client build: {e}"))?;
        Ok(Self { client, auth })
    }
}

impl ModelClient for HttpModelClient {
    fn complete(
        &self,
        spec: &ModelSpec,
        messages: Vec<ModelMessage>,
    ) -> Result<Completion, String> {
        let url = format!("{}/chat/completions", spec.base_url.trim_end_matches('/'));
        // extra_body first: provider knobs may join, never shadow identity.
        let mut body = match &spec.extra_body {
            Some(Value::Object(map)) => map.clone(),
            Some(other) => return Err(format!("extra_body is {other}, not a table")),
            None => Map::new(),
        };
        body.insert("model".into(), json!(spec.model));
        body.insert(
            "messages".into(),
            Value::Array(
                messages
                    .iter()
                    .map(|m| json!({ "role": m.role, "content": m.content }))
                    .collect(),
            ),
        );
        let mut request = self.client.post(url).json(&Value::Object(body));
        if let Some(key) = &self.auth {
            request = request.bearer_auth(key);
        }
        let response = request.send().map_err(|e| format!("model call: {e}"))?;
        let status = response.status();
        let text = response
            .text()
            .map_err(|e| format!("model response unreadable: {e}"))?;
        if !status.is_success() {
            let snippet: String = text.chars().take(ERROR_SNIPPET).collect();
            return Err(format!("model http {status}: {snippet}"));
        }
        let value: Value =
            serde_json::from_str(&text).map_err(|e| format!("model response not json: {e}"))?;
        let content = value["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| "model response carries no content".to_string())?;
        let usage = value
            .get("usage")
            .and_then(Value::as_object)
            .ok_or_else(|| "model response carries no usage".to_string())?;
        let input = usage["prompt_tokens"]
            .as_u64()
            .ok_or_else(|| "model usage carries no prompt_tokens".to_string())?;
        let output = usage["completion_tokens"]
            .as_u64()
            .ok_or_else(|| "model usage carries no completion_tokens".to_string())?;
        Ok(Completion {
            content: content.to_string(),
            usage: Some(Usage {
                input_tokens: input,
                output_tokens: output,
            }),
        })
    }
}
