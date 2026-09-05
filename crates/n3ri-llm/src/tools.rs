//! OpenAI 兼容 tool calling wire 类型（无 Bevy 依赖，可单测）。
//!
//! 协议形状抄 N.E.K.O `main_logic/tool_calling.py` 的 OpenAI-flavoured 约定：
//! 请求 `{"type":"function","function":{name,description,parameters}}`，
//! 模型返回 `assistant.tool_calls[]`，执行后回填 `role:"tool"` 消息。
//! 多轮循环（max 3）由调用方（n3ri-agent turn.rs，M4）驱动；本模块只做单次
//! `send_with_tools` + 纯函数解析/序列化。

use serde::{Deserialize, Serialize};

use crate::{LlmClient, LlmConfig, Message};

// ============================
//  类型
// ============================

#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCallRequest {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Default)]
pub struct AssistantMessage {
    pub content: String,
    pub tool_calls: Vec<ToolCallRequest>,
}

impl AssistantMessage {
    pub fn is_empty(&self) -> bool {
        self.content.trim().is_empty() && self.tool_calls.is_empty()
    }
}

/// tool 执行结果包络：永不抛异常，错误也包在里面回模型自我纠正。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolEnvelope {
    pub output: serde_json::Value,
    pub is_error: bool,
}

impl ToolEnvelope {
    pub fn ok(output: serde_json::Value) -> Self {
        Self { output, is_error: false }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            output: serde_json::json!({ "error": msg.into() }),
            is_error: true,
        }
    }

    /// 进 `role:"tool"` 消息 content 的 JSON 字符串。
    pub fn wire_content(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| r#"{"output":null,"is_error":true}"#.into())
    }
}

// ============================
//  纯函数：序列化 / 解析
// ============================

pub fn tool_def_to_openai(tool: &ToolDef) -> serde_json::Value {
    serde_json::json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.parameters,
        }
    })
}

/// arguments 解析：`{…}` 对象原样；非对象包进 `{"value":…}`；
/// 非法 JSON / 空进 `{"_raw":…}` / `{}`。永不失败。
pub fn parse_tool_arguments(raw: Option<&str>) -> serde_json::Value {
    let Some(s) = raw else {
        return serde_json::json!({});
    };
    if s.trim().is_empty() {
        return serde_json::json!({});
    }
    match serde_json::from_str::<serde_json::Value>(s) {
        Ok(serde_json::Value::Object(_)) => serde_json::from_str(s).unwrap_or_default(),
        Ok(v) => serde_json::json!({ "value": v }),
        Err(_) => serde_json::json!({ "_raw": s }),
    }
}

/// 解析 `choices[0].message`：content 可能是空字符串或 null（纯 tool_calls 回复时合法）。
pub fn parse_assistant_message(msg: &serde_json::Value) -> AssistantMessage {
    let content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let mut tool_calls = Vec::new();
    if let Some(calls) = msg.get("tool_calls").and_then(|v| v.as_array()) {
        for call in calls {
            let Some(name) = call
                .pointer("/function/name")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            else {
                continue;
            };
            let id = call
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let raw_args = call.pointer("/function/arguments").and_then(|v| v.as_str());
            tool_calls.push(ToolCallRequest {
                id,
                name: name.to_string(),
                arguments: parse_tool_arguments(raw_args),
            });
        }
    }
    AssistantMessage { content, tool_calls }
}

/// 终答前把含 tool_calls 的 assistant 消息回填进历史（供下一轮调用）。
pub fn assistant_to_wire(assistant: &AssistantMessage) -> serde_json::Value {
    serde_json::json!({
        "role": "assistant",
        "content": assistant.content,
        "tool_calls": assistant.tool_calls.iter().map(|c| {
            serde_json::json!({
                "id": c.id,
                "type": "function",
                "function": {
                    "name": c.name,
                    "arguments": serde_json::to_string(&c.arguments).unwrap_or_default(),
                }
            })
        }).collect::<Vec<_>>(),
    })
}

pub fn tool_to_wire(call: &ToolCallRequest, envelope: &ToolEnvelope) -> serde_json::Value {
    serde_json::json!({
        "role": "tool",
        "tool_call_id": call.id,
        "name": call.name,
        "content": envelope.wire_content(),
    })
}

// ============================
//  请求体
// ============================

fn build_body_from_wire(
    messages: &[serde_json::Value],
    config: &LlmConfig,
    tools: &[ToolDef],
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "model": config.model,
        "messages": messages,
        "max_tokens": config.max_tokens,
        "temperature": config.temperature,
        "stream": false,
        "tools": tools.iter().map(tool_def_to_openai).collect::<Vec<_>>(),
        "tool_choice": "auto",
    });
    if config.disable_thinking {
        body["thinking"] = serde_json::json!({ "type": "disabled" });
    }
    body
}

impl LlmClient {
    fn post_json(
        &self,
        url: &str,
        body: &serde_json::Value,
        config: &LlmConfig,
    ) -> Result<serde_json::Value, String> {
        let resp = self
            .http
            .post(url)
            .header("Authorization", format!("Bearer {}", config.api_key))
            .json(body)
            .send()
            .map_err(|e| format!("连接失败: {e}"))?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().unwrap_or_default();
            return Err(format!("API 错误 {status}: {text}"));
        }
        resp.json().map_err(|e| format!("响应 JSON 无效: {e}"))
    }

    /// 带 tools 的阻塞单次调用（多轮循环由调用方驱动）。
    /// 纯 tool_calls 回复（content 空）是合法返回；两者皆空才报错。
    pub fn send_with_tools(
        &self,
        messages: &[Message],
        config: &LlmConfig,
        tools: &[ToolDef],
    ) -> Result<AssistantMessage, String> {
        let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
        let wire: Vec<serde_json::Value> = messages
            .iter()
            .map(|m| {
                serde_json::json!({
                    "role": serde_json::to_value(m.role).unwrap_or_default(),
                    "content": m.content,
                })
            })
            .collect();
        let json = self.post_json(&url, &build_body_from_wire(&wire, config, tools), config)?;
        let assistant = parse_assistant_message(&json["choices"][0]["message"]);
        if assistant.is_empty() {
            let dump = serde_json::to_string(&json).unwrap_or_default();
            let snippet: String = dump.chars().take(500).collect();
            eprintln!("n3ri-llm: 模型返回空 content 且无 tool_calls,响应结构: {snippet}");
            return Err(format!("模型返回空 content(响应片段: {snippet})"));
        }
        Ok(assistant)
    }
}

// ============================
//  内置 tool schema
// ============================

pub fn recall_memory_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "query": { "type": "string", "description": "自然语言检索，如“用户喜欢什么游戏”" },
            "time": { "type": "string", "description": "可选时间窗，如“上周”、“2026-08”" }
        },
        "required": []
    })
}

pub fn open_app_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "app_id": { "type": "string", "description": "伪桌面应用 id，如 terminal/files/browser/settings" }
        },
        "required": ["app_id"]
    })
}

pub fn notify_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "title": { "type": "string", "description": "通知标题（可选）" },
            "message": { "type": "string", "description": "通知正文" }
        },
        "required": ["message"]
    })
}

pub fn niri_windows_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {},
        "required": []
    })
}

pub fn niri_spawn_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "command": { "type": "string", "description": "白名单内的程序名，如 firefox/kitty/nautilus" },
            "args": { "type": "array", "items": { "type": "string" }, "description": "可选参数" }
        },
        "required": ["command"]
    })
}

pub fn niri_window_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "action": { "type": "string", "enum": ["focus"], "description": "只读切换（close 暂不暴露）" },
            "window_id": { "type": "integer", "description": "niri 窗口 id（来自 niri_windows）" }
        },
        "required": ["action", "window_id"]
    })
}

pub fn builtin_tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "recall_memory",
            description: "检索 Nori 的温记忆（facts/reflections）。想不起用户的事时调用。",
            parameters: recall_memory_schema(),
        },
        ToolDef {
            name: "open_app",
            description: "打开伪桌面内置应用窗口（如 terminal/files/browser/settings）。",
            parameters: open_app_schema(),
        },
        ToolDef {
            name: "notify",
            description: "发一条桌面通知（提醒类事项）。",
            parameters: notify_schema(),
        },
        ToolDef {
            name: "niri_windows",
            description: "列出 niri 合成器中的真实窗口（id/title/app_id/focused）。只读。",
            parameters: niri_windows_schema(),
        },
        ToolDef {
            name: "niri_spawn",
            description: "在 niri 中启动白名单内的真实应用。command 必须在白名单里。",
            parameters: niri_spawn_schema(),
        },
        ToolDef {
            name: "niri_window",
            description: "按 id 聚焦一个 niri 真实窗口。",
            parameters: niri_window_schema(),
        },
    ]
}

// ============================
//  单测
// ============================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_def_serializes_to_openai_shape() {
        let def = ToolDef {
            name: "recall_memory",
            description: "查记忆",
            parameters: recall_memory_schema(),
        };
        let v = tool_def_to_openai(&def);
        assert_eq!(v["type"], serde_json::json!("function"));
        assert_eq!(v["function"]["name"], serde_json::json!("recall_memory"));
        assert_eq!(
            v["function"]["parameters"]["type"],
            serde_json::json!("object")
        );
    }

    #[test]
    fn parse_text_only_message() {
        let msg = serde_json::json!({ "role": "assistant", "content": "你好呀。" });
        let a = parse_assistant_message(&msg);
        assert_eq!(a.content, "你好呀。");
        assert!(a.tool_calls.is_empty());
        assert!(!a.is_empty());
    }

    #[test]
    fn parse_tool_calls_with_null_content() {
        let msg = serde_json::json!({
            "role": "assistant",
            "content": null,
            "tool_calls": [
                {
                    "id": "call_1",
                    "type": "function",
                    "function": {
                        "name": "recall_memory",
                        "arguments": "{\"query\":\"用户喜欢什么游戏\"}"
                    }
                },
                {
                    "id": "call_2",
                    "type": "function",
                    "function": { "name": "", "arguments": "{}" }
                }
            ]
        });
        let a = parse_assistant_message(&msg);
        assert_eq!(a.content, "");
        assert_eq!(a.tool_calls.len(), 1);
        assert_eq!(a.tool_calls[0].id, "call_1");
        assert_eq!(a.tool_calls[0].name, "recall_memory");
        assert_eq!(
            a.tool_calls[0].arguments["query"],
            serde_json::json!("用户喜欢什么游戏")
        );
        assert!(!a.is_empty());
    }

    #[test]
    fn illegal_arguments_fall_back_without_error() {
        assert_eq!(parse_tool_arguments(None), serde_json::json!({}));
        assert_eq!(parse_tool_arguments(Some("  ")), serde_json::json!({}));
        assert_eq!(
            parse_tool_arguments(Some("{not json")),
            serde_json::json!({ "_raw": "{not json" })
        );
        assert_eq!(
            parse_tool_arguments(Some("[1,2]")),
            serde_json::json!({ "value": [1, 2] })
        );
    }

    #[test]
    fn wire_roundtrip_keeps_ids() {
        let call = ToolCallRequest {
            id: "call_abc".into(),
            name: "niri_windows".into(),
            arguments: serde_json::json!({}),
        };
        let assistant = AssistantMessage {
            content: String::new(),
            tool_calls: vec![call.clone()],
        };
        let aw = assistant_to_wire(&assistant);
        assert_eq!(aw["role"], serde_json::json!("assistant"));
        assert_eq!(aw["tool_calls"][0]["id"], serde_json::json!("call_abc"));
        let tw = tool_to_wire(&call, &ToolEnvelope::ok(serde_json::json!({"windows":[]})));
        assert_eq!(tw["role"], serde_json::json!("tool"));
        assert_eq!(tw["tool_call_id"], serde_json::json!("call_abc"));
        assert!(!tw["content"].as_str().unwrap_or("").is_empty());
    }

    #[test]
    fn error_envelope_marks_is_error() {
        let env = ToolEnvelope::err("不在白名单");
        assert!(env.is_error);
        let content: serde_json::Value =
            serde_json::from_str(&env.wire_content()).expect("envelope 可序列化");
        assert!(content["is_error"].as_bool().unwrap_or(false));
    }

    #[test]
    fn schema_snapshots() {
        for schema in [
            recall_memory_schema(),
            open_app_schema(),
            notify_schema(),
            niri_windows_schema(),
            niri_spawn_schema(),
            niri_window_schema(),
        ] {
            assert_eq!(schema["type"], serde_json::json!("object"));
        }
        assert_eq!(
            open_app_schema()["required"],
            serde_json::json!(["app_id"])
        );
        assert_eq!(
            niri_window_schema()["properties"]["action"]["enum"],
            serde_json::json!(["focus"])
        );
        assert_eq!(builtin_tool_defs().len(), 6);
    }
}
