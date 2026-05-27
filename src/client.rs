//! Minimal HTTP+SSE model client.
//!
//! Source map:
//! - `build_responses_request` : codex-rs/core/src/client.rs:709
//! - `ModelClientSession::stream` (SSE branch only)
//!                            : codex-rs/core/src/client.rs:1547
//! - `Accept: text/event-stream` header
//!                            : codex-rs/codex-api/src/endpoint/responses.rs:139
//! - `spawn_response_stream`  : codex-rs/codex-api/src/sse/responses.rs:29
//!
//! The WebSocket branch (`responses_websocket_enabled` path in client.rs:1561)
//! is intentionally omitted; this reproduction only exercises HTTP+SSE.

use std::time::Duration;

use futures::StreamExt;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderValue};
use tokio::sync::mpsc;

use crate::sse::{process_chat_sse, process_sse};
use crate::types::{
    ApiError, ByteStream, ChatCompletionsRequest, ChatMessage, ContentItem, Prompt, ResponseEvent,
    ResponseItem, ResponsesApiRequest,
};

/// Mirrors: codex-rs/core/src/client.rs:1712
/// `const RESPONSE_STREAM_CHANNEL_CAPACITY: usize = 1600;`
/// (Kept smaller here because a single "hello" reply needs only a handful of
/// deltas.)
const RESPONSE_STREAM_CHANNEL_CAPACITY: usize = 64;

pub struct ModelClient {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub http: reqwest::Client,
    pub idle_timeout: Duration,
}

impl ModelClient {
    pub fn new(api_key: String, base_url: String, model: String) -> Self {
        Self {
            api_key,
            base_url,
            model,
            http: reqwest::Client::new(),
            // codex's default is 5 min; we use 30 s for a fast-fail demo.
            // codex source: codex-rs/model-provider-info/src/lib.rs:26
            // `DEFAULT_STREAM_IDLE_TIMEOUT_MS = 300_000`.
            idle_timeout: Duration::from_secs(30),
        }
    }

    /// Mirrors: codex-rs/core/src/client.rs:709  `build_responses_request`
    /// Translates the host-side `Prompt` into the OpenAI wire payload.
    pub fn build_responses_request(&self, prompt: &Prompt) -> ResponsesApiRequest {
        // Mirrors: codex-rs/core/src/client.rs:738
        // `create_text_param_for_request(verbosity, schema, strict)` —
        // simplified to only carry the structured-output schema when present.
        let text = prompt.output_schema.as_ref().map(|schema| {
            serde_json::json!({
                "format": {
                    "type": "json_schema",
                    "strict": prompt.output_schema_strict,
                    "schema": schema,
                }
            })
        });
        ResponsesApiRequest {
            model: self.model.clone(),
            instructions: prompt.base_instructions.text.clone(),
            input: prompt.input.clone(),
            tools: prompt.tools.clone(),
            parallel_tool_calls: prompt.parallel_tool_calls,
            stream: true,
            // Mirrors line 754:
            // `store: provider.is_azure_responses_endpoint()`.
            // For plain OpenAI / OpenAI-compatible endpoints this is `false`,
            // matching codex's choice for the ChatGPT-auth + API-key paths.
            store: false,
            text,
        }
    }

    /// Mirrors: codex-rs/core/src/client.rs:1547  `ModelClientSession::stream`
    /// (HTTP+SSE branch only).
    ///
    /// Returns an `mpsc::Receiver<Result<ResponseEvent, ApiError>>` populated
    /// by a background task running `process_sse` — same pattern as
    /// `spawn_response_stream` in codex-rs/codex-api/src/sse/responses.rs:29.
    pub async fn stream(
        &self,
        prompt: &Prompt,
    ) -> Result<mpsc::Receiver<Result<ResponseEvent, ApiError>>, ApiError> {
        let body = self.build_responses_request(prompt);
        // OpenAI SDK convention: `base_url` already carries the API version
        // prefix (e.g. `/v1`), so we only append `/responses` here. Set
        // `OPENAI_BASE_URL=https://api.openai.com/v1` for the official endpoint.
        let url = format!("{}/responses", self.base_url.trim_end_matches('/'));

        // Mirrors: codex-rs/codex-api/src/endpoint/responses.rs:139
        // `headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));`
        let response = self
            .http
            .post(&url)
            .header(AUTHORIZATION, format!("Bearer {}", self.api_key))
            .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
            .header(ACCEPT, HeaderValue::from_static("text/event-stream"))
            .json(&body)
            .send()
            .await
            .map_err(|err| ApiError::Stream(format!("request failed: {err}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(ApiError::Stream(format!("HTTP {status}: {text}")));
        }

        // Reqwest's `bytes_stream()` returns the body as
        // `impl Stream<Item = Result<Bytes, reqwest::Error>>`; we box it into
        // codex's `ByteStream` alias (see types.rs) before handing it to
        // `process_sse`, matching codex-rs/codex-api/src/sse/responses.rs:807
        // `let stream: ByteStream = Box::pin(stream);`.
        let byte_stream: ByteStream = response.bytes_stream().boxed();

        // Mirrors: codex-rs/codex-api/src/sse/responses.rs:29  `spawn_response_stream`
        let (tx_event, rx_event) = mpsc::channel(RESPONSE_STREAM_CHANNEL_CAPACITY);
        let idle = self.idle_timeout;
        tokio::spawn(async move {
            process_sse(byte_stream, tx_event, idle).await;
        });
        Ok(rx_event)
    }

    /// Extension beyond codex: stream via `POST /v1/chat/completions`.
    /// Same overall shape as `stream` but uses the chat completions wire
    /// format so we can talk to providers that don't host the Responses API
    /// (Tencent LKE, DeepSeek, Qwen, OpenRouter chat-only models, etc.).
    pub async fn stream_chat(
        &self,
        prompt: &Prompt,
    ) -> Result<mpsc::Receiver<Result<ResponseEvent, ApiError>>, ApiError> {
        let messages = build_chat_messages(prompt);
        let body = ChatCompletionsRequest {
            model: self.model.clone(),
            messages,
            stream: true,
        };
        // Same convention as `stream`: caller's `base_url` carries the API
        // version prefix; we only append `/chat/completions`. Mirrors the
        // OpenAI Python/Node SDKs (e.g. Tencent LKE uses
        // `https://api.lkeap.cloud.tencent.com/plan/v3` as the prefix).
        let url = format!(
            "{}/chat/completions",
            self.base_url.trim_end_matches('/')
        );

        let response = self
            .http
            .post(&url)
            .header(AUTHORIZATION, format!("Bearer {}", self.api_key))
            .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
            .header(ACCEPT, HeaderValue::from_static("text/event-stream"))
            .json(&body)
            .send()
            .await
            .map_err(|err| ApiError::Stream(format!("request failed: {err}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(ApiError::Stream(format!("HTTP {status}: {text}")));
        }

        let byte_stream: ByteStream = response.bytes_stream().boxed();
        let (tx_event, rx_event) = mpsc::channel(RESPONSE_STREAM_CHANNEL_CAPACITY);
        let idle = self.idle_timeout;
        tokio::spawn(async move {
            process_chat_sse(byte_stream, tx_event, idle).await;
        });
        Ok(rx_event)
    }
}

/// Translate the codex-style `Prompt` (`Vec<ResponseItem>` + system text) into
/// the OpenAI chat-completions `messages` array.
fn build_chat_messages(prompt: &Prompt) -> Vec<ChatMessage> {
    let mut messages = Vec::new();
    if !prompt.base_instructions.text.is_empty() {
        messages.push(ChatMessage {
            role: "system".to_string(),
            content: prompt.base_instructions.text.clone(),
        });
    }
    for item in &prompt.input {
        match item {
            ResponseItem::Message { role, content } => {
                let text = content
                    .iter()
                    .filter_map(|c| match c {
                        ContentItem::InputText { text } | ContentItem::OutputText { text } => {
                            Some(text.as_str())
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("");
                messages.push(ChatMessage {
                    role: role.clone(),
                    content: text,
                });
            }
        }
    }
    messages
}
