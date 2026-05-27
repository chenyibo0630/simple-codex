//! Wire-format types.
//!
//! Each struct/enum mirrors its codex counterpart; the trimmed-down shape only
//! carries the fields needed for a single "hello" round-trip over SSE.
//!
//! Source map:
//! - `Prompt`                : codex-rs/core/src/client_common.rs:25
//! - `ResponseItem`/`ContentItem`
//!                           : codex-rs/protocol/src/models.rs
//! - `ResponsesApiRequest`   : codex-rs/protocol/src/openai_models.rs
//!                             (constructed by `build_responses_request` at
//!                              codex-rs/core/src/client.rs:709)
//! - `ResponsesStreamEvent`,
//!   `ResponseEvent`,
//!   `ResponseCompleted`,
//!   `ApiError`              : codex-rs/codex-api/src/sse/responses.rs
//! - `ByteStream`            : codex-rs/codex-client/src/transport.rs:18

use bytes::Bytes;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Mirrors: codex-rs/codex-client/src/transport.rs:18
/// `pub type ByteStream = BoxStream<'static, Result<Bytes, TransportError>>;`
/// We carry `reqwest::Error` directly because this minimal version skips the
/// transport abstraction layer.
pub type ByteStream = BoxStream<'static, Result<Bytes, reqwest::Error>>;

/// Mirrors: codex-rs/core/src/client_common.rs:25  `pub struct Prompt`
#[derive(Debug, Clone)]
pub struct Prompt {
    /// Conversation context input items.
    pub input: Vec<ResponseItem>,

    /// Tools available to the model, including additional tools sourced from
    /// external MCP servers.
    pub(crate) tools: Vec<ToolSpec>,

    /// Whether parallel tool calls are permitted for this prompt.
    pub(crate) parallel_tool_calls: bool,

    pub base_instructions: BaseInstructions,

    /// Optionally specify the personality of the model.
    /// Kept for structural parity with codex; not yet wired into wire format.
    #[allow(dead_code)]
    pub personality: Option<Personality>,

    /// Optional the output schema for the model's response.
    pub output_schema: Option<Value>,

    /// Whether the Responses API should strictly validate `output_schema`.
    pub output_schema_strict: bool,
}

/// Mirrors: codex-rs/core/src/client_common.rs:48  `impl Default for Prompt`
/// `output_schema_strict` defaults to `true` to match codex.
impl Default for Prompt {
    fn default() -> Self {
        Self {
            input: Vec::new(),
            tools: Vec::new(),
            parallel_tool_calls: false,
            base_instructions: BaseInstructions::default(),
            personality: None,
            output_schema: None,
            output_schema_strict: true,
        }
    }
}

/// Mirrors: codex-rs/core/src/client_common.rs  `BaseInstructions`
/// codex's version wraps the system-prompt string with the `text` field; it is
/// produced by `Session::get_base_instructions` at
/// codex-rs/core/src/session/mod.rs:1120-1125.
#[derive(Debug, Clone, Default)]
pub struct BaseInstructions {
    pub text: String,
}

/// Mirrors: codex-rs/protocol/src/config_types.rs  `enum Personality`
/// Simplified — codex's enum carries personality presets that get baked into
/// the system prompt via `model_info.get_model_instructions(personality)`
/// (codex-rs/core/src/context_manager/history.rs:139).
/// Not part of the Responses API wire format itself.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Personality {
    Default,
    Concise,
    Friendly,
}

/// Mirrors: codex-rs/tools/src/tool_spec.rs:17  `pub enum ToolSpec`
/// Only the `Function` variant is implemented here; codex also has
/// Namespace / ToolSearch / ImageGeneration / WebSearch / Freeform.
/// `Function` is unused in the hello path but kept so `Prompt.tools` has a
/// real element type to point at.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolSpec {
    Function {
        name: String,
        description: String,
        strict: bool,
        /// codex uses a typed `JsonSchema` AST
        /// (codex-rs/tools/src/json_schema.rs:34); we accept raw JSON here.
        parameters: Value,
    },
}

/// Mirrors: codex-rs/protocol/src/models.rs  `enum ResponseItem`
/// Only the `Message` variant is implemented here; the full enum also covers
/// FunctionCall / FunctionCallOutput / Reasoning / etc.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ResponseItem {
    #[serde(rename = "message")]
    Message {
        role: String,
        content: Vec<ContentItem>,
    },
}

/// Mirrors: codex-rs/protocol/src/models.rs  `enum ContentItem`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentItem {
    #[serde(rename = "input_text")]
    InputText { text: String },
    #[serde(rename = "output_text")]
    OutputText { text: String },
}

/// Mirrors: codex-rs/protocol/src/openai_models.rs  `ResponsesApiRequest`
/// (built in codex-rs/core/src/client.rs:746 inside `build_responses_request`).
/// Codex's full struct also carries `tool_choice`, `reasoning`, `include`,
/// `service_tier`, `prompt_cache_key`, `client_metadata`; those are dropped
/// here because the hello payload does not need them.
#[derive(Debug, Clone, Serialize)]
pub struct ResponsesApiRequest {
    pub model: String,
    pub instructions: String,
    pub input: Vec<ResponseItem>,
    /// Skip when empty so a hello call does not send `"tools": []`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolSpec>,
    pub parallel_tool_calls: bool,
    pub stream: bool,
    pub store: bool,
    /// Structured-output config; codex builds this via
    /// `create_text_param_for_request` (codex-rs/core/src/client.rs:738).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<Value>,
}

// =============================================================================
// Chat Completions support (extension beyond codex)
// -----------------------------------------------------------------------------
// codex itself only speaks the Responses API; `wire_api = "chat"` was removed
// (see codex-rs/model-provider-info/src/lib.rs:45 `CHAT_WIRE_API_REMOVED_ERROR`).
// We add a minimal chat-completions path so simple-codex can talk to providers
// that don't expose `/v1/responses` (Tencent LKE, DeepSeek, Qwen, etc).
// =============================================================================

/// Request payload for `POST /v1/chat/completions` (OpenAI-compatible).
#[derive(Debug, Clone, Serialize)]
pub struct ChatCompletionsRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub stream: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// One SSE chunk from chat completions. Shape:
/// `{"id":"...","choices":[{"delta":{"content":"你"},"index":0,"finish_reason":null}]}`
#[derive(Debug, Deserialize)]
pub struct ChatStreamChunk {
    #[serde(default)]
    pub id: Option<String>,
    pub choices: Vec<ChatChunkChoice>,
}

#[derive(Debug, Deserialize)]
pub struct ChatChunkChoice {
    #[serde(default)]
    pub delta: ChatChunkDelta,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ChatChunkDelta {
    #[serde(default)]
    pub content: Option<String>,
}

/// Mirrors: codex-rs/codex-api/src/sse/responses.rs  `ResponsesStreamEvent`
/// The raw SSE envelope decoded by `process_responses_event` (line 263).
#[derive(Debug, Deserialize)]
pub struct ResponsesStreamEvent {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub delta: Option<String>,
    #[serde(default)]
    pub response: Option<serde_json::Value>,
}

/// Mirrors: codex-rs/codex-api/src/sse/responses.rs  `enum ResponseEvent`
/// Only the subset needed to print a streamed "hello" reply is kept.
#[derive(Debug)]
pub enum ResponseEvent {
    Created,
    OutputTextDelta(String),
    Completed { response_id: String },
}

/// Mirrors: codex-rs/codex-api/src/sse/responses.rs  `ResponseCompleted`
#[derive(Debug, Deserialize)]
pub struct ResponseCompleted {
    pub id: String,
}

/// Mirrors: codex-rs/codex-api/src/sse/responses.rs  `enum ApiError`
/// Only the `Stream` variant is preserved; the original distinguishes
/// ContextWindowExceeded / QuotaExceeded / Retryable / etc.
#[derive(Debug)]
pub enum ApiError {
    Stream(String),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::Stream(msg) => write!(f, "stream error: {msg}"),
        }
    }
}

impl std::error::Error for ApiError {}
