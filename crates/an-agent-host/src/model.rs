//! The real model client: OpenAI-compatible chat completions over HTTP.
//! Synchronous at the core boundary, cancellable async HTTP inside the host.
//! Async callers must use a blocking worker, not a runtime executor thread.
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

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, Ordering},
};
use std::time::Duration;

use serde_json::{Map, Value, json};

use an_agent_core::control::{Completion, ModelClient, ModelMessage, Usage};
use an_agent_core::principal::card::ModelSpec;

/// Model calls are slow; a hung one is not forever. Connect fails fast,
/// the whole call gets five minutes.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const CALL_TIMEOUT: Duration = Duration::from_secs(300);

const RESPONSE_LIMIT: u64 = 1024 * 1024;

/// One approved attempt. Clone to observe settlement; never reuse for retries.
#[derive(Clone)]
pub struct ModelCallControl {
    cancelled: Arc<AtomicBool>,
    timeout: Duration,
    state: Arc<AtomicU8>,
}

impl ModelCallControl {
    /// A caller-owned cancellation flag and a bounded whole-request deadline.
    pub fn new(cancelled: Arc<AtomicBool>, timeout: Duration) -> Result<Self, String> {
        if timeout.is_zero() || timeout > CALL_TIMEOUT {
            return Err("model timeout must be in (0, 300 seconds]".into());
        }
        Ok(Self {
            cancelled,
            timeout,
            state: Arc::new(AtomicU8::new(0)),
        })
    }

    /// Actual local attempt settlement, not the current signal flag or remote state.
    pub fn status(&self) -> &'static str {
        match self.state.load(Ordering::SeqCst) {
            0 => "not_started",
            1 => "running",
            2 => "succeeded",
            3 => "cancelled",
            4 => "timed_out",
            _ => "failed",
        }
    }
}

/// A blocking HTTP [`ModelClient`] for OpenAI-compatible endpoints
/// (`POST {base_url}/chat/completions`). Bearer auth is the host's to
/// give — the key never lives on a card, so the tape never records it.
pub struct HttpModelClient {
    #[cfg(test)]
    resolver: Option<Arc<dns_tests::PendingDns>>,
    auth: Option<String>,
    control: Option<ModelCallControl>,
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
        Ok(Self {
            #[cfg(test)]
            resolver: None,
            auth,
            control: None,
        })
    }

    /// Bind a single controlled attempt without changing core's synchronous trait.
    pub fn with_control(mut self, control: ModelCallControl) -> Self {
        self.control = Some(control);
        self
    }
}

impl ModelClient for HttpModelClient {
    fn complete(
        &self,
        spec: &ModelSpec,
        messages: Vec<ModelMessage>,
    ) -> Result<Completion, String> {
        if tokio::runtime::Handle::try_current().is_ok() {
            return Err(
                "synchronous model client requires a blocking worker outside the async runtime"
                    .into(),
            );
        }
        // Own the runtime and the future: interruption drops both before return.
        // Never detach a blocking HTTP worker that keeps generating after cancellation.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "model runtime unavailable".to_string())?;
        let control = self.control.clone().unwrap_or(ModelCallControl::new(
            Arc::new(AtomicBool::new(false)),
            CALL_TIMEOUT,
        )?);
        control
            .state
            .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| "model attempt already used; no automatic retry".to_string())?;
        let (state, result) = runtime.block_on(async {
            let cancelled = async {
                loop {
                    if control.cancelled.load(Ordering::SeqCst) { break; }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            };
            tokio::select! {
                biased;
                _ = cancelled => (3, Err("model request cancelled".into())),
                _ = tokio::time::sleep(control.timeout) => (4, Err("model request timed out".into())),
                result = self.request(spec, messages) => {
                    (if result.is_ok() {2} else {5}, result)
                }
            }
        });
        control.state.store(state, Ordering::SeqCst);
        result
    }
}

impl HttpModelClient {
    async fn request(
        &self,
        spec: &ModelSpec,
        messages: Vec<ModelMessage>,
    ) -> Result<Completion, String> {
        // Resolver and connection state belong to this runtime/attempt. Reusing
        // them across destroyed runtimes can retain unusable DNS connections.
        // Async DNS avoids Tokio's non-cancellable blocking getaddrinfo workers.
        let builder = reqwest::Client::builder()
            .hickory_dns(true)
            .connect_timeout(CONNECT_TIMEOUT)
            .retry(reqwest::retry::never())
            .pool_max_idle_per_host(0)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy();
        #[cfg(test)]
        let builder = match &self.resolver {
            Some(resolver) => builder.dns_resolver(resolver.clone()),
            None => builder,
        };
        let client = builder
            .build()
            .map_err(|_| "model client unavailable".to_string())?;
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
        let mut request = client.post(url).json(&Value::Object(body));
        if let Some(key) = &self.auth {
            request = request.bearer_auth(key);
        }
        let mut response = request
            .send()
            .await
            .map_err(|_| "model transport failed".to_string())?;
        let status = response.status();
        if !status.is_success() {
            // Provider error bodies may echo credentials or private input.
            return Err(format!("model http {status}"));
        }
        let mut text = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "model response unreadable".to_string())?
        {
            if (text.len() + chunk.len()) as u64 > RESPONSE_LIMIT {
                return Err("model response exceeds limit".into());
            }
            text.extend_from_slice(&chunk);
        }
        let value: Value =
            serde_json::from_slice(&text).map_err(|_| "model response not json".to_string())?;
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

#[cfg(test)]
#[path = "model_dns_tests.rs"]
mod dns_tests;
