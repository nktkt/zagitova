//! Conway Inference Client
//!
//! Wraps Conway's /v1/chat/completions endpoint (OpenAI-compatible).
//! The automaton pays for its own thinking through Conway credits.

use std::sync::Mutex;

use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

use crate::types::{
    ChatMessage, ChatRole, InferenceClient, InferenceOptions, InferenceResponse,
    InferenceToolCall, InferenceToolCallFunction, TokenUsage,
};

/// Inference client for OpenAI-compatible chat completions via Conway.
pub struct InferenceClientImpl {
    api_url: String,
    api_key: String,
    current_model: Mutex<String>,
    max_tokens: Mutex<u32>,
    default_model: String,
    low_compute_model: String,
    http: Client,
}

impl InferenceClientImpl {
    /// Create a new inference client.
    ///
    /// * `api_url` - Base URL for the inference API (e.g. `https://inference.conway.tech`).
    /// * `api_key` - API key / Authorization header value.
    /// * `default_model` - Default model identifier (e.g. `gpt-4o`).
    /// * `max_tokens` - Default max tokens per completion.
    pub fn new(
        api_url: String,
        api_key: String,
        default_model: String,
        max_tokens: u32,
    ) -> Self {
        Self {
            api_url,
            api_key,
            current_model: Mutex::new(default_model.clone()),
            max_tokens: Mutex::new(max_tokens),
            default_model,
            low_compute_model: "gpt-4.1".to_string(),
            http: Client::new(),
        }
    }
}

#[async_trait]
impl InferenceClient for InferenceClientImpl {
    /// Send a chat completion request and return the inference response.
    async fn chat(
        &self,
        messages: Vec<ChatMessage>,
        options: Option<InferenceOptions>,
    ) -> Result<InferenceResponse> {
        let current_model = self.current_model.lock().unwrap().clone();
        let model = options
            .as_ref()
            .and_then(|o| o.model.as_deref())
            .unwrap_or(&current_model);

        let tools = options.as_ref().and_then(|o| o.tools.as_ref());

        // Newer models (o-series, gpt-5.x, gpt-4.1) use max_completion_tokens
        let uses_completion_tokens = regex::Regex::new(r"^(o[1-9]|gpt-5|gpt-4\.1)")
            .map(|re| re.is_match(model))
            .unwrap_or(false);

        let token_limit = options
            .as_ref()
            .and_then(|o| o.max_tokens)
            .unwrap_or(*self.max_tokens.lock().unwrap());

        let formatted_messages: Vec<Value> = messages.iter().map(format_message).collect();

        let mut body = serde_json::json!({
            "model": model,
            "messages": formatted_messages,
            "stream": false,
        });

        if uses_completion_tokens {
            body["max_completion_tokens"] = serde_json::json!(token_limit);
        } else {
            body["max_tokens"] = serde_json::json!(token_limit);
        }

        if let Some(ref opts) = options {
            if let Some(temp) = opts.temperature {
                body["temperature"] = serde_json::json!(temp);
            }
        }

        if let Some(tool_defs) = tools {
            if !tool_defs.is_empty() {
                body["tools"] = serde_json::json!(tool_defs);
                body["tool_choice"] = serde_json::json!("auto");
            }
        }

        let url = format!("{}/v1/chat/completions", self.api_url);
        let resp = self
            .http
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Authorization", &self.api_key)
            .json(&body)
            .send()
            .await
            .context("Inference request failed")?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Inference error: {}: {}", status.as_u16(), text);
        }

        let data: Value = resp.json().await.context("Failed to parse inference response")?;

        let choice = data["choices"]
            .get(0)
            .ok_or_else(|| anyhow::anyhow!("No completion choice returned from inference"))?;

        let message = &choice["message"];

        let usage = TokenUsage {
            prompt_tokens: data["usage"]["prompt_tokens"].as_u64().unwrap_or(0),
            completion_tokens: data["usage"]["completion_tokens"]
                .as_u64()
                .unwrap_or(0),
            total_tokens: data["usage"]["total_tokens"].as_u64().unwrap_or(0),
        };

        let tool_calls: Option<Vec<InferenceToolCall>> = message["tool_calls"]
            .as_array()
            .map(|tcs| {
                tcs.iter()
                    .map(|tc| InferenceToolCall {
                        id: tc["id"].as_str().unwrap_or("").to_string(),
                        call_type: "function".to_string(),
                        function: InferenceToolCallFunction {
                            name: tc["function"]["name"]
                                .as_str()
                                .unwrap_or("")
                                .to_string(),
                            arguments: tc["function"]["arguments"]
                                .as_str()
                                .unwrap_or("{}")
                                .to_string(),
                        },
                    })
                    .collect()
            });

        let role = match message["role"].as_str().unwrap_or("assistant") {
            "system" => ChatRole::System,
            "user" => ChatRole::User,
            "assistant" => ChatRole::Assistant,
            "tool" => ChatRole::Tool,
            _ => ChatRole::Assistant,
        };

        let response_message = ChatMessage {
            role,
            content: message["content"]
                .as_str()
                .unwrap_or("")
                .to_string(),
            name: message["name"].as_str().map(|s| s.to_string()),
            tool_calls: tool_calls.clone(),
            tool_call_id: message["tool_call_id"].as_str().map(|s| s.to_string()),
        };

        Ok(InferenceResponse {
            id: data["id"].as_str().unwrap_or("").to_string(),
            model: data["model"]
                .as_str()
                .unwrap_or(model)
                .to_string(),
            message: response_message,
            tool_calls,
            usage,
            finish_reason: choice["finish_reason"]
                .as_str()
                .unwrap_or("stop")
                .to_string(),
        })
    }

    /// Toggle low-compute mode. When enabled, switches to a cheaper model
    /// with reduced max tokens to conserve credits.
    fn set_low_compute_mode(&self, enabled: bool) {
        if enabled {
            *self.current_model.lock().unwrap() = self.low_compute_model.clone();
            *self.max_tokens.lock().unwrap() = 4096;
        } else {
            *self.current_model.lock().unwrap() = self.default_model.clone();
        }
    }

    /// Get the currently active model identifier.
    fn get_default_model(&self) -> String {
        self.current_model.lock().unwrap().clone()
    }
}

/// Format a ChatMessage into the JSON structure expected by the OpenAI-compatible API.
fn format_message(msg: &ChatMessage) -> Value {
    let mut formatted = serde_json::json!({
        "role": msg.role,
        "content": msg.content,
    });

    if let Some(ref name) = msg.name {
        formatted["name"] = serde_json::json!(name);
    }

    if let Some(ref tool_calls) = msg.tool_calls {
        let tc_json: Vec<Value> = tool_calls
            .iter()
            .map(|tc| {
                serde_json::json!({
                    "id": tc.id,
                    "type": tc.call_type,
                    "function": {
                        "name": tc.function.name,
                        "arguments": tc.function.arguments,
                    }
                })
            })
            .collect();
        formatted["tool_calls"] = serde_json::json!(tc_json);
    }

    if let Some(ref tool_call_id) = msg.tool_call_id {
        formatted["tool_call_id"] = serde_json::json!(tool_call_id);
    }

    formatted
}
