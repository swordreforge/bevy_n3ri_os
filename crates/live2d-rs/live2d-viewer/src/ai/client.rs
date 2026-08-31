use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::time::Duration;

use crate::ai::types::{
    AiConfig, AiStreamEvent, ChatMessage, ChatRole, ToolCall, ToolCallFunction, ToolDefinition,
};

/// OpenAI-compatible chat completion client.
///
/// Uses `reqwest::blocking` — the winit event loop is synchronous, so
/// `block_on` over an async client buys nothing for MVP. The blocking client
/// sends the HTTP request on the current thread; the UI shows a `Thinking...`
/// state so the user knows progress is happening.
pub struct AiChatClient {
    http: reqwest::blocking::Client,
}

/// Response from a non-streaming chat completion.
pub struct ChatResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
}

impl AiChatClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .expect("failed to create HTTP client"),
        }
    }

    /// Serialize a ChatMessage into the OpenAI API JSON format.
    fn serialize_message(m: &ChatMessage) -> serde_json::Value {
        let role_str = match m.role {
            ChatRole::User => "user",
            ChatRole::Assistant => "assistant",
            ChatRole::System => "system",
            ChatRole::Tool => "tool",
        };
        let mut obj = serde_json::json!({ "role": role_str });

        match m.role {
            ChatRole::Tool => {
                obj["content"] = serde_json::Value::String(m.content.clone());
                if let Some(ref id) = m.tool_call_id {
                    obj["tool_call_id"] = serde_json::Value::String(id.clone());
                }
            }
            ChatRole::Assistant => {
                if let Some(ref tool_calls) = m.tool_calls {
                    if !tool_calls.is_empty() {
                        obj["content"] = serde_json::Value::Null;
                        obj["tool_calls"] = serde_json::to_value(tool_calls).unwrap_or_default();
                    } else {
                        obj["content"] = serde_json::Value::String(m.content.clone());
                    }
                } else {
                    obj["content"] = serde_json::Value::String(m.content.clone());
                }
            }
            _ => {
                obj["content"] = serde_json::Value::String(m.content.clone());
            }
        }
        obj
    }

    /// Build the common JSON body for chat completion requests.
    fn build_body(
        messages: &[ChatMessage],
        config: &AiConfig,
        stream: bool,
        tools: Option<&[ToolDefinition]>,
    ) -> serde_json::Value {
        let mut body = serde_json::json!({
            "model": config.model,
            "messages": messages.iter().map(Self::serialize_message).collect::<Vec<_>>(),
            "max_tokens": config.max_tokens,
            "temperature": config.temperature,
            "stream": stream,
        });
        if let Some(tools) = tools {
            body["tools"] = serde_json::to_value(tools).unwrap_or_default();
            body["tool_choice"] = serde_json::json!("auto");
            body["parallel_tool_calls"] = serde_json::json!(false);
        }
        body
    }

    /// Post a request and check the HTTP status.
    fn post(
        &self,
        url: &str,
        body: &serde_json::Value,
        config: &AiConfig,
    ) -> Result<reqwest::blocking::Response, String> {
        let resp = self
            .http
            .post(url)
            .header("Authorization", format!("Bearer {}", config.api_key))
            .json(body)
            .send()
            .map_err(|e| format!("Connection failed: {e}"))?;

        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().unwrap_or_default();
            return Err(format!("API error {status}: {body_text}"));
        }
        Ok(resp)
    }

    /// Serialize the message slice into the OpenAI chat completions request,
    /// POST it to `{base_url}/chat/completions`, and return the assistant's
    /// reply text on success.
    pub fn send(&self, messages: &[ChatMessage], config: &AiConfig) -> Result<String, String> {
        let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
        let body = Self::build_body(messages, config, false, None);
        let resp = self.post(&url, &body, config)?;

        let json: serde_json::Value = resp
            .json()
            .map_err(|e| format!("Invalid response JSON: {e}"))?;

        let content: String = json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| "Empty response from model".to_string())?
            .to_string();

        Ok(content)
    }

    /// Send a chat completion request with `"stream": true` and emit parsed
    /// SSE events into the sender. Runs on a background thread.
    ///
    /// When `tools` is provided, the request includes tool definitions and
    /// the SSE stream is parsed for `delta.tool_calls` in addition to
    /// `delta.content`. Complete tool calls are emitted as `ToolCall` events.
    pub fn send_stream(
        &self,
        messages: &[ChatMessage],
        config: &AiConfig,
        tools: Option<&[ToolDefinition]>,
        tx: std::sync::mpsc::Sender<AiStreamEvent>,
    ) {
        let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
        let body = Self::build_body(messages, config, true, tools);

        let resp = match self.post(&url, &body, config) {
            Ok(r) => r,
            Err(e) => {
                let _ = tx.send(AiStreamEvent::Error(e));
                return;
            }
        };

        let reader = BufReader::new(resp);
        let mut tool_acc: HashMap<usize, (Option<String>, Option<String>, String)> = HashMap::new();

        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    let _ = tx.send(AiStreamEvent::Error(format!("Read error: {e}")));
                    return;
                }
            };
            let Some(data) = line.strip_prefix("data: ") else {
                continue;
            };
            if data == "[DONE]" {
                break;
            }
            let Ok(json) = serde_json::from_str::<serde_json::Value>(data) else {
                continue;
            };

            // Parse delta.content tokens
            if let Some(content) = json["choices"][0]["delta"]["content"].as_str() {
                if !content.is_empty() {
                    let _ = tx.send(AiStreamEvent::Token(content.to_string()));
                }
            }

            // Parse delta.tool_calls (streaming — may arrive over multiple chunks)
            if let Some(tcs) = json["choices"][0]["delta"]["tool_calls"].as_array() {
                for tc in tcs {
                    let index = tc["index"].as_u64().unwrap_or(0) as usize;
                    let entry = tool_acc
                        .entry(index)
                        .or_insert_with(|| (None, None, String::new()));

                    if let Some(id) = tc["id"].as_str() {
                        entry.0 = Some(id.to_string());
                    }
                    if let Some(name) = tc["function"]["name"].as_str() {
                        entry.1 = Some(name.to_string());
                    }
                    if let Some(args) = tc["function"]["arguments"].as_str() {
                        entry.2.push_str(args);
                    }
                }
            }
        }

        // Emit accumulated tool calls
        for (_idx, (id, name, args)) in tool_acc.drain() {
            if let (Some(id), Some(name)) = (id, name) {
                let _ = tx.send(AiStreamEvent::ToolCall(ToolCall {
                    id,
                    type_: "function".to_string(),
                    function: ToolCallFunction {
                        name,
                        arguments: args,
                    },
                }));
            }
        }

        // Only emit Done if no tool calls were sent (tool calls signal done
        // via the event itself; the receiver should stop after receiving them)
        let _ = tx.send(AiStreamEvent::Done);
    }

    /// Non-streaming chat completion with tool definitions.
    ///
    /// Sends messages + tools and returns either text content or tool calls.
    /// Used for the multi-turn tool calling loop (re-request after tool
    /// execution results).
    pub fn send_with_tools(
        &self,
        messages: &[ChatMessage],
        config: &AiConfig,
        tools: &[ToolDefinition],
    ) -> Result<ChatResponse, String> {
        let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
        let body = Self::build_body(messages, config, false, Some(tools));
        let resp = self.post(&url, &body, config)?;

        let json: serde_json::Value = resp
            .json()
            .map_err(|e| format!("Invalid response JSON: {e}"))?;

        let message = &json["choices"][0]["message"];
        let content = message["content"].as_str().map(|s| s.to_string());

        let tool_calls: Vec<ToolCall> = message["tool_calls"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|tc| {
                        Some(ToolCall {
                            id: tc["id"].as_str()?.to_string(),
                            type_: "function".to_string(),
                            function: ToolCallFunction {
                                name: tc["function"]["name"].as_str()?.to_string(),
                                arguments: tc["function"]["arguments"].as_str()?.to_string(),
                            },
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(ChatResponse {
            content,
            tool_calls,
        })
    }

    #[cfg(feature = "capture")]
    #[allow(dead_code)]
    pub fn send_vision(
        &self,
        messages: &[ChatMessage],
        frame_base64: &str,
        prompt: &str,
        config: &AiConfig,
    ) -> Result<String, String> {
        let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));

        let mut api_messages: Vec<serde_json::Value> =
            messages.iter().map(Self::serialize_message).collect();

        let vision_msg = serde_json::json!({
            "role": "user",
            "content": [
                {"type": "text", "text": prompt},
                {"type": "image_url", "image_url": {"url": frame_base64, "detail": "low"}}
            ]
        });
        api_messages.push(vision_msg);

        let body = serde_json::json!({
            "model": config.model,
            "messages": api_messages,
            "max_tokens": config.max_tokens,
            "temperature": config.temperature,
            "stream": false,
        });

        let resp = self.post(&url, &body, config)?;
        let json: serde_json::Value = resp.json().map_err(|e| format!("JSON: {e}"))?;
        json["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "Empty vision response".to_string())
    }

    /// Send a minimal "ping" to verify the provider configuration is working.
    pub fn test_connection(&self, config: &AiConfig) -> Result<String, String> {
        let msg = ChatMessage {
            role: ChatRole::User,
            content: "Respond with exactly one word: ok".into(),
            timestamp: 0.0,
            tool_call_id: None,
            tool_calls: None,
        };
        self.send(&[msg], config)
    }
}
