//! 内部抽象类型。
//!
//! 这里只放 cli / app-server 直接构造或消费的"业务"类型；
//! 上游 OpenAI/Chat-Completions 的 wire-format DTO 留在 `simple-codex-client::wire`。
//!
//! Source map:
//! - `Prompt`               : codex-rs/core/src/client_common.rs:25
//! - `ResponseItem`/`ContentItem`
//!                          : codex-rs/protocol/src/models.rs
//! - `ResponseEvent`,
//!   `ApiError`             : codex-rs/codex-api/src/sse/responses.rs

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Mirrors: codex-rs/core/src/client_common.rs:25  `pub struct Prompt`
#[derive(Debug, Clone)]
pub struct Prompt {
    /// Conversation context input items.
    pub input: Vec<ResponseItem>,

    /// Tools available to the model, including additional tools sourced from
    /// external MCP servers.
    pub tools: Vec<ToolSpec>,

    /// Whether parallel tool calls are permitted for this prompt.
    pub parallel_tool_calls: bool,

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
#[derive(Debug, Clone, Default)]
pub struct BaseInstructions {
    pub text: String,
}

/// Mirrors: codex-rs/protocol/src/config_types.rs  `enum Personality`
/// Simplified — codex's enum carries personality presets that get baked into
/// the system prompt via `model_info.get_model_instructions(personality)`.
/// Not part of the Responses API wire format itself.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Personality {
    Default,
    Concise,
    Friendly,
}

/// Mirrors: codex-rs/tools/src/tool_spec.rs:17  `pub enum ToolSpec`
/// Only the `Function` variant is implemented here.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolSpec {
    Function {
        name: String,
        description: String,
        strict: bool,
        parameters: Value,
    },
}

/// Mirrors: codex-rs/protocol/src/models.rs  `enum ResponseItem`
/// Only the `Message` variant is implemented here.
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

/// Mirrors: codex-rs/codex-api/src/sse/responses.rs  `enum ResponseEvent`
/// Only the subset needed to print a streamed "hello" reply is kept.
#[derive(Debug)]
pub enum ResponseEvent {
    Created,
    OutputTextDelta(String),
    Completed { response_id: String },
}

/// Mirrors: codex-rs/codex-api/src/sse/responses.rs  `enum ApiError`
/// Only the `Stream` variant is preserved.
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
