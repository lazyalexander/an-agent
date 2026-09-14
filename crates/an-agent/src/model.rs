use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::act::{Tool, ToolCall};
use crate::agent::{AgentError, Assistant, ChatMessage, Model, WireToolCall};

#[derive(Debug, Clone)]
pub struct ModelSettings {
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    pub extra_body: Option<Value>,
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
    async fn complete(
        &self,
        messages: &[ChatMessage],
        tools: &[Arc<dyn Tool>],
    ) -> Result<Assistant, AgentError> {
        let url = format!(
            "{}/chat/completions",
            self.settings.base_url.trim_end_matches('/')
        );
        let mut body = json!({
            "model": self.settings.model,
            "messages": messages,
        });
        if !tools.is_empty() {
            let listed: Vec<Value> = tools
                .iter()
                .map(|t| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": t.name(),
                            "description": t.description(),
                            "parameters": t.parameters(),
                        }
                    })
                })
                .collect();
            body["tools"] = Value::Array(listed);
        }
        if let Some(extra) = &self.settings.extra_body {
            if let (Some(base), Some(over)) = (body.as_object_mut(), extra.as_object()) {
                for (k, v) in over {
                    base.insert(k.clone(), v.clone());
                }
            }
        }
        let mut req = self.client.post(url).json(&body);
        if let Some(key) = &self.settings.api_key {
            req = req.bearer_auth(key);
        }
        let resp = req.send().await.map_err(|e| AgentError::Model(e.to_string()))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(AgentError::Model(format!("{status}: {text}")));
        }
        let parsed: ChatResponse = resp.json().await.map_err(|e| AgentError::Model(e.to_string()))?;
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
                    crate::kit::Entropy::os().ulid(crate::kit::Clock::wall().now_ms()).to_string()
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
        })
    }
}

pub fn load_settings() -> Result<ModelSettings, AgentError> {
    let extra = std::fs::read_to_string("config/model.json")
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok());
    let file_base = extra
        .as_ref()
        .and_then(|v| v.get("baseUrl"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let file_model = extra
        .as_ref()
        .and_then(|v| v.get("model"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let extra_body = extra.as_ref().and_then(|v| v.get("extraBody")).cloned();
    let base_url = std::env::var("MODEL_BASE_URL")
        .ok()
        .or(file_base)
        .ok_or_else(|| AgentError::Model("MODEL_BASE_URL or config/model.json baseUrl required".into()))?;
    let model = std::env::var("MODEL_NAME")
        .ok()
        .or(file_model)
        .ok_or_else(|| AgentError::Model("MODEL_NAME or config/model.json model required".into()))?;
    let api_key = std::env::var("MODEL_API_KEY").ok();
    Ok(ModelSettings {
        base_url,
        model,
        api_key,
        extra_body,
    })
}
