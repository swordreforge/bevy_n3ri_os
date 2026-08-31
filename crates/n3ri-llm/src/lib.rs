//! n3ri_os LLM 基础设施:OpenAI 兼容 chat completion 客户端。
//!
//! 参考 live2d-viewer 的 ai 模块,只保留游戏所需的最小面:
//! 阻塞式请求、后台线程流式请求(SSE)、配置持久化。
//! 工具调用 / 视觉 / TTS 待具体游戏需要时再扩展。

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::Duration;

use serde::{Deserialize, Serialize};

// ============================
//  消息类型
// ============================
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: Role::System, content: content.into() }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: Role::User, content: content.into() }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: Role::Assistant, content: content.into() }
    }
}

// ============================
//  配置与持久化
// ============================
fn default_disable_thinking() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// API 基地址(如 "http://localhost:11434/v1" 或 "https://api.openai.com/v1")
    pub base_url: String,
    /// API Key(本地 Ollama 可留空)
    pub api_key: String,
    /// 模型名(如 "llama3.2"、"gpt-4o-mini")
    pub model: String,
    /// 单次回复最大 token 数
    pub max_tokens: u32,
    /// 采样温度 0.0–2.0
    pub temperature: f32,
    /// 关闭思考模式。DeepSeek V4 等混合推理模型默认 thinking=enabled,
    /// 思考 token 计入 max_tokens 且显著拖慢响应;结构化输出场景建议关闭。
    /// 旧配置文件缺失该字段时默认 true。
    #[serde(default = "default_disable_thinking")]
    pub disable_thinking: bool,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:11434/v1".into(),
            api_key: String::new(),
            model: "llama3.2".into(),
            max_tokens: 2048,
            temperature: 0.7,
            disable_thinking: true,
        }
    }
}

fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("n3ri_os")
}

pub fn config_path() -> PathBuf {
    config_dir().join("llm-config.json")
}

/// 从 $CONFIG_DIR/n3ri_os/llm-config.json 读取;缺失或损坏时返回默认值
pub fn load_config() -> LlmConfig {
    match std::fs::read_to_string(config_path()) {
        Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
        Err(_) => LlmConfig::default(),
    }
}

/// 原子写入(temp + rename)
pub fn save_config(config: &LlmConfig) {
    let dir = config_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = config_path();
    let tmp = dir.join("llm-config.json.tmp");
    match serde_json::to_string_pretty(config) {
        Ok(json) => {
            if std::fs::write(&tmp, &json).is_ok() {
                let _ = std::fs::rename(&tmp, &path);
            }
        }
        Err(e) => eprintln!("n3ri-llm: 序列化配置失败: {e}"),
    }
}

// ============================
//  流式事件
// ============================
#[derive(Debug, Clone)]
pub enum StreamEvent {
    Token(String),
    Done,
    Error(String),
}

// ============================
//  客户端
// ============================
pub struct LlmClient {
    http: reqwest::blocking::Client,
}

impl Default for LlmClient {
    fn default() -> Self {
        Self::new()
    }
}

impl LlmClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .expect("n3ri-llm: HTTP 客户端构建失败"),
        }
    }

    fn serialize_message(m: &Message) -> serde_json::Value {
        serde_json::json!({
            "role": serde_json::to_value(m.role).unwrap_or_default(),
            "content": m.content,
        })
    }

    fn build_body(messages: &[Message], config: &LlmConfig, stream: bool) -> serde_json::Value {
        let mut body = serde_json::json!({
            "model": config.model,
            "messages": messages.iter().map(Self::serialize_message).collect::<Vec<_>>(),
            "max_tokens": config.max_tokens,
            "temperature": config.temperature,
            "stream": stream,
        });
        if config.disable_thinking {
            // DeepSeek V4 官方开关:thinking 默认 enabled,思考 token 计入 max_tokens,
            // 预算耗尽会返回 HTTP 200 但 content 为空。
            // 非 DeepSeek 端点若拒绝该字段,可在 llm-config.json 设 "disable_thinking": false
            body["thinking"] = serde_json::json!({ "type": "disabled" });
        }
        body
    }

    fn post(
        &self,
        url: &str,
        body: &serde_json::Value,
        config: &LlmConfig,
    ) -> Result<reqwest::blocking::Response, String> {
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
        Ok(resp)
    }

    /// 阻塞式单轮补全,返回助手回复文本
    pub fn send(&self, messages: &[Message], config: &LlmConfig) -> Result<String, String> {
        let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
        let resp = self.post(&url, &Self::build_body(messages, config, false), config)?;
        let json: serde_json::Value = resp.json().map_err(|e| format!("响应 JSON 无效: {e}"))?;
        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("");
        if content.trim().is_empty() {
            // 空回复(如 reasoner 模型把 token 花在 reasoning_content、或 finish_reason=length)
            // 时附带响应结构片段,便于定位
            let dump = serde_json::to_string(&json).unwrap_or_default();
            let snippet: String = dump.chars().take(500).collect();
            eprintln!("n3ri-llm: 模型返回空 content,响应结构: {snippet}");
            return Err(format!(
                "模型返回空 content(响应片段: {snippet})"
            ));
        }
        Ok(content.to_string())
    }

    /// SSE 流式补全,在后台线程运行,向 tx 发送 Token/Done/Error 事件
    pub fn send_stream(
        &self,
        messages: &[Message],
        config: &LlmConfig,
        tx: Sender<StreamEvent>,
    ) {
        let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
        let resp = match self.post(&url, &Self::build_body(messages, config, true), config) {
            Ok(r) => r,
            Err(e) => {
                let _ = tx.send(StreamEvent::Error(e));
                return;
            }
        };
        let reader = BufReader::new(resp);
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    let _ = tx.send(StreamEvent::Error(format!("读取失败: {e}")));
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
            if let Some(content) = json["choices"][0]["delta"]["content"].as_str() {
                if !content.is_empty() {
                    let _ = tx.send(StreamEvent::Token(content.to_string()));
                }
            }
        }
        let _ = tx.send(StreamEvent::Done);
    }

    /// 最小 ping 验证配置可用
    pub fn test_connection(&self, config: &LlmConfig) -> Result<String, String> {
        let msg = Message::user("Respond with exactly one word: ok");
        self.send(&[msg], config)
    }
}
