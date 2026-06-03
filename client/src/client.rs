//! Minimal HTTP+SSE model client.
//!
//! Source map:
//! - `ModelClient`            : codex-rs/core/src/client.rs (持有 Provider)
//! - `build_responses_request`: codex-rs/core/src/client.rs:709
//! - `stream` dispatch        : codex-rs/core/src/client.rs:1547
//!                              (按 `provider.wire_api` 选 Responses/Chat 分支)
//! - SSE 启动                 : codex-rs/codex-api/src/sse/responses.rs:29
//!
//! WebSocket / 重试 / fallback 路径全部省略。

use std::time::Duration;

use futures::StreamExt;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderValue};
use tokio::sync::mpsc;

use simple_codex_core::config::WireApi;
use simple_codex_core::types::{
    ApiError, ContentItem, Prompt, ResponseEvent, ResponseItem,
};

use crate::provider::Provider;
use crate::sse::{process_chat_sse, process_sse};
use crate::wire::{ByteStream, ChatCompletionsRequest, ChatMessage, ResponsesApiRequest};

/// Mirrors: codex-rs/core/src/client.rs:1712
/// `const RESPONSE_STREAM_CHANNEL_CAPACITY: usize = 1600;`
const RESPONSE_STREAM_CHANNEL_CAPACITY: usize = 64;

pub struct ModelClient {
    pub api_key: String,
    pub provider: Provider,
    pub model: String,
    pub http: reqwest::Client,
    pub idle_timeout: Duration,
}

impl ModelClient {
    pub fn new(api_key: String, provider: Provider, model: String) -> Self {
        Self {
            api_key,
            provider,
            model,
            http: reqwest::Client::new(),
            // codex 默认 5 min;这里 30 s 让 demo fail-fast。
            // codex source: codex-rs/model-provider-info/src/lib.rs:26
            idle_timeout: Duration::from_secs(30),
        }
    }

    /// 统一流式入口。按 `provider.wire_api` 内部分发到 Responses / Chat。
    ///
    /// 对齐 codex-rs/core/src/client.rs:1547 `ModelClientSession::stream` —— 调用方
    /// 永远只看到一个 `stream`,protocol 选择由 client 持有的 provider 决定。
    pub async fn stream(
        &self,
        prompt: &Prompt,
    ) -> Result<mpsc::Receiver<Result<ResponseEvent, ApiError>>, ApiError> {
        match self.provider.wire_api {
            WireApi::Responses => self.stream_responses(prompt).await,
            WireApi::Chat => self.stream_chat(prompt).await,
        }
    }

    /// Mirrors: codex-rs/core/src/client.rs:709 `build_responses_request`
    fn build_responses_request(&self, prompt: &Prompt) -> ResponsesApiRequest {
        // Mirrors: codex-rs/core/src/client.rs:738 `create_text_param_for_request`
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
            // Mirrors line 754: `store: provider.is_azure_responses_endpoint()`
            store: false,
            text,
        }
    }

    /// POST `/responses` 分支。
    async fn stream_responses(
        &self,
        prompt: &Prompt,
    ) -> Result<mpsc::Receiver<Result<ResponseEvent, ApiError>>, ApiError> {
        let body = self.build_responses_request(prompt);
        let url = format!(
            "{}/responses",
            self.provider.base_url.trim_end_matches('/')
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

        // Mirrors: codex-rs/codex-api/src/sse/responses.rs:29 `spawn_response_stream`
        let (tx_event, rx_event) = mpsc::channel(RESPONSE_STREAM_CHANNEL_CAPACITY);
        let idle = self.idle_timeout;
        tokio::spawn(async move {
            process_sse(byte_stream, tx_event, idle).await;
        });
        Ok(rx_event)
    }

    /// POST `/chat/completions` 分支(codex 之外的扩展)。
    async fn stream_chat(
        &self,
        prompt: &Prompt,
    ) -> Result<mpsc::Receiver<Result<ResponseEvent, ApiError>>, ApiError> {
        let messages = build_chat_messages(prompt);
        let body = ChatCompletionsRequest {
            model: self.model.clone(),
            messages,
            stream: true,
        };
        let url = format!(
            "{}/chat/completions",
            self.provider.base_url.trim_end_matches('/')
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

/// 把 codex 风格 `Prompt` (`Vec<ResponseItem>` + system text) 翻译成 OpenAI
/// chat-completions 的 `messages` 数组。
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
