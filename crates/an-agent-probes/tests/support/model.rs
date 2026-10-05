//! Chat-completions client for live probes.

use super::*;

// --- chat-completions HTTP client (live probes only) ---

#[derive(Debug, Clone)]
pub struct ModelSettings {
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    pub extra_body: Option<serde_json::Value>,
}

#[derive(Clone)]
pub struct ChatCompletions {
    client: reqwest::Client,
    settings: ModelSettings,
}

impl ChatCompletions {
    pub fn new(settings: ModelSettings) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(180))
                .build()
                .expect("reqwest client"),
            settings,
        }
    }
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    usage: Option<WireUsage>,
}

#[derive(Deserialize)]
struct WireUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
}

#[derive(Deserialize)]
struct Choice {
    message: WireAssistant,
}

#[derive(Deserialize)]
struct WireAssistant {
    content: Option<String>,
    tool_calls: Option<Vec<WireToolCall>>,
}

impl Model for ChatCompletions {
    fn spec(&self) -> serde_json::Value {
        serde_json::json!({
            "base_url": self.settings.base_url,
            "model": self.settings.model,
            "extra_body": self.settings.extra_body,
        })
    }

    async fn complete(
        &self,
        messages: &[ChatMessage],
        tools: &[Arc<dyn Tool>],
    ) -> Result<Assistant, AgentError> {
        let url = format!(
            "{}/chat/completions",
            self.settings.base_url.trim_end_matches('/')
        );
        let mut body = serde_json::json!({
            "model": self.settings.model,
            "messages": messages,
        });
        if !tools.is_empty() {
            let listed: Vec<serde_json::Value> = tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": t.name(),
                            "description": t.description(),
                            "parameters": t.parameters(),
                        }
                    })
                })
                .collect();
            body["tools"] = serde_json::Value::Array(listed);
        }
        if let Some(extra) = &self.settings.extra_body
            && let (Some(base), Some(over)) = (body.as_object_mut(), extra.as_object())
        {
            for (k, v) in over {
                base.insert(k.clone(), v.clone());
            }
        }
        let mut req = self.client.post(url).json(&body);
        if let Some(key) = &self.settings.api_key {
            req = req.bearer_auth(key);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| AgentError::Model(e.to_string()))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(AgentError::Model(format!("{status}: {text}")));
        }
        let parsed: ChatResponse = resp
            .json()
            .await
            .map_err(|e| AgentError::Model(e.to_string()))?;
        let msg = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| AgentError::Model("empty choices".into()))?
            .message;
        let tool_calls = msg
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .map(|c| ToolCall {
                id: if c.id.is_empty() {
                    an_agent_core::det_seam::Entropy::os()
                        .ulid(an_agent_core::det_seam::Clock::wall().now_ms())
                        .to_string()
                } else {
                    c.id
                },
                name: c.function.name,
                arguments: c.function.arguments,
            })
            .collect();
        Ok(Assistant {
            content: msg.content.unwrap_or_default(),
            tool_calls,
            usage: parsed.usage.map(|u| Usage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
            }),
        })
    }
}

// --- shared temp dir guard (integration tests cannot use src testkit) ---
