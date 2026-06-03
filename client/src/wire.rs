//! 上游 wire-format DTO（OpenAI Responses + Chat Completions）。
//!
//! 仅在 client crate 内部使用——`ModelClient` 构造请求体、`sse` 解响应体。
//! 不通过 `lib.rs` 对外暴露。
//!
//! Source map:
//! - `ResponsesApiRequest`   : codex-rs/protocol/src/openai_models.rs
//!                             (constructed by `build_responses_request` at
//!                              codex-rs/core/src/client.rs:709)
//! - `ResponsesStreamEvent`,
//!   `ResponseCompleted`     : codex-rs/codex-api/src/sse/responses.rs
//! - `ByteStream`            : codex-rs/codex-client/src/transport.rs:18

use bytes::Bytes;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use simple_codex_core::types::{ResponseItem, ToolSpec};

/// Mirrors: codex-rs/codex-client/src/transport.rs:18
/// `pub type ByteStream = BoxStream<'static, Result<Bytes, TransportError>>;`
pub(crate) type ByteStream = BoxStream<'static, Result<Bytes, reqwest::Error>>;

/// Mirrors: codex-rs/protocol/src/openai_models.rs  `ResponsesApiRequest`
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ResponsesApiRequest {
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
// =============================================================================

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ChatCompletionsRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub stream: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// One SSE chunk from chat completions. Shape:
/// `{"id":"...","choices":[{"delta":{"content":"你"},"index":0,"finish_reason":null}]}`
#[derive(Debug, Deserialize)]
pub(crate) struct ChatStreamChunk {
    #[serde(default)]
    pub id: Option<String>,
    pub choices: Vec<ChatChunkChoice>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChatChunkChoice {
    #[serde(default)]
    pub delta: ChatChunkDelta,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct ChatChunkDelta {
    #[serde(default)]
    pub content: Option<String>,
}

/// Mirrors: codex-rs/codex-api/src/sse/responses.rs  `ResponsesStreamEvent`
#[derive(Debug, Deserialize)]
pub(crate) struct ResponsesStreamEvent {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub delta: Option<String>,
    #[serde(default)]
    pub response: Option<serde_json::Value>,
}

/// Mirrors: codex-rs/codex-api/src/sse/responses.rs  `ResponseCompleted`
#[derive(Debug, Deserialize)]
pub(crate) struct ResponseCompleted {
    pub id: String,
}
