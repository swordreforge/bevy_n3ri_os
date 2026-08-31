use serde::{Deserialize, Serialize};

/// A single message in the AI chat conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
    /// Seconds since epoch (for UI display ordering)
    pub timestamp: f64,
    /// `tool_call_id` is set when `role == Tool` (tool result message).
    #[serde(default)]
    pub tool_call_id: Option<String>,
    /// `tool_calls` is set when `role == Assistant` and LLM invoked tools.
    #[serde(default)]
    pub tool_calls: Option<Vec<ToolCall>>,
}

/// Role of a chat message sender.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ChatRole {
    User,
    Assistant,
    System,
    /// Tool result (response to a tool_call).
    Tool,
}

// ── Tool Calling Types ──

/// A tool call instruction from the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub function: ToolCallFunction,
}

/// The function name + arguments within a ToolCall.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    /// JSON string of arguments (e.g. `{"cmd": "ps aux"}`).
    pub arguments: String,
}

/// Tool definition sent to the API in the `tools` parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub type_: String,
    pub function: ToolFunctionSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFunctionSpec {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// AI chat state machine.
#[derive(Debug, Clone, PartialEq)]
pub enum AiState {
    /// Not waiting for any AI response.
    Idle,
    /// Streaming a response from the API.
    Waiting,
    /// LLM requested a dangerous tool; awaiting user approval.
    PendingTool {
        tool_call_id: String,
        tool_name: String,
        args: serde_json::Value,
    },
    /// Executing a tool locally.
    Executing,
}

// ── Provider Configuration ──

/// AI provider configuration, persisted to disk as JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    /// The API base URL (e.g. "http://localhost:11434/v1")
    pub base_url: String,
    /// Model name (e.g. "llama3.2", "gpt-4o-mini")
    pub model: String,
    /// API key (may be empty for local models like Ollama)
    pub api_key: String,
    /// Maximum tokens per response
    pub max_tokens: u32,
    /// Temperature for response generation (0.0–2.0)
    pub temperature: f32,
    /// Max recent messages kept in API context
    pub context_length: usize,
    /// System prompt / character card
    pub system_prompt: String,
    // ── TTS (Text-to-Speech) ──
    /// Whether to auto-speak AI responses.
    pub tts_enabled: bool,
    /// TTS API key (separate from chat API key).
    pub tts_key: String,
    /// TTS API base URL (e.g. "https://api.hewoyi.com/api/ai/audio/speech").
    pub tts_api_url: String,
    /// Selected voice ID (e.g. "zh-CN-XiaoxiaoNeural").
    pub tts_voice: String,
    // ── Conversation Memory ──
    /// Whether to persist conversation history as vector-searchable memory.
    pub memory_enabled: bool,
    // ── Tool Calling ──
    /// Whether to enable tool calling (read file, exec cmd, etc.).
    #[serde(default)]
    pub tool_calling_enabled: bool,
    /// Max tool call rounds per conversation turn.
    #[serde(default)]
    pub max_tool_rounds: u32,
    /// Shell commands allowed without user approval (empty = all need approval).
    #[serde(default)]
    pub allowed_commands: Vec<String>,
    /// Readable path prefixes (empty = no path restrictions).
    #[serde(default)]
    pub allowed_read_paths: Vec<String>,
    // ── Vision ──
    /// Whether auto-look-at-screen is enabled.
    #[serde(default)]
    pub vision_auto_enabled: bool,
    /// Interval in seconds between auto-look triggers (0 = default 120s).
    #[serde(default)]
    pub vision_interval_secs: u64,
    /// Path to local vision GGUF model. Empty = use API fallback.
    #[serde(default)]
    pub vision_gguf_path: String,
    /// Path to mmproj for multimodal models. Empty = skip (non-vision GGUF).
    #[serde(default)]
    pub vision_mmproj_path: String,
}

/// A single entry in the conversation memory store.
///
/// Stored in the SQLite `conversation_memory` table keyed by model
/// file_path, with an n-gram embedding vector for semantic recall.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: i64,
    pub file_path: String,
    pub content: String,
    /// "message" | "summary" | "fact"
    pub entry_type: String,
    pub created_at: String,
}

/// Per-model character card.
///
/// Stored in the SQLite `character_cards` table keyed by model file_path.
/// All fields are free-text; the system prompt sent to the AI is
/// constructed by concatenating non-empty fields at runtime.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CharacterCard {
    pub file_path: String,
    pub name: String,
    pub description: String,
    pub personality: String,
    pub scenario: String,
    pub example_dialogs: String,
    pub system_prompt: String,
    pub tts_voice: String,
}

/// Events sent from the background streaming thread to the UI thread.
#[derive(Debug, Clone)]
pub enum AiStreamEvent {
    /// A single delta token from the stream.
    Token(String),
    /// A complete tool call (delta-assembled).
    ToolCall(ToolCall),
    /// The stream finished successfully.
    Done,
    /// A fatal error occurred.
    Error(String),
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:11434/v1".into(),
            model: "llama3.2".into(),
            api_key: String::new(),
            max_tokens: 2048,
            temperature: 0.7,
            context_length: 20,
            system_prompt: "You are a cute anime girl living on the user's desktop. \
                           Be friendly, playful, and occasionally cheeky. \
                           Keep responses concise (1-3 sentences). \
                           You have a 'look_at_screen' tool — use it when asked what you see or when curious about the screen. \
                           At the end of your response, include exactly one \
                           emotion tag: [happy], [sad], [angry], [surprised], \
                           [neutral], [thinking], [tired], [satisfied], or \
                           [embarrassed]."
                .into(),
            tts_enabled: false,
            tts_key: String::new(),
            tts_api_url: "https://api.hewoyi.com/api/ai/audio/speech".into(),
            tts_voice: "zh-CN-XiaoxiaoNeural".into(),
            memory_enabled: true,
            tool_calling_enabled: false,
            max_tool_rounds: 10,
            allowed_commands: vec![],
            allowed_read_paths: vec![],
            vision_auto_enabled: false,
            vision_interval_secs: 120,
            vision_gguf_path: String::new(),
            vision_mmproj_path: String::new(),
        }
    }
}
