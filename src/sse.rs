//! SSE pipeline.
//!
//! Source map:
//! - `process_responses_event` : codex-rs/codex-api/src/sse/responses.rs:263
//! - `process_sse`             : codex-rs/codex-api/src/sse/responses.rs:399
//!
//! The idle-timeout `tokio::time::timeout(idle_timeout, stream.next())` pattern
//! and the "stream closed before response.completed" sentinel are taken
//! verbatim from codex.

use std::time::Duration;

use eventsource_stream::Eventsource;
use futures::StreamExt;
use tokio::sync::mpsc;
use tokio::time::timeout;

use crate::types::{
    ApiError, ByteStream, ChatStreamChunk, ResponseCompleted, ResponseEvent, ResponsesStreamEvent,
};

/// Mirrors: codex-rs/codex-api/src/sse/responses.rs:263  `process_responses_event`
///
/// Only the kinds needed for a "hello" turn are decoded; everything else is
/// silently ignored (codex handles output_item.added/done, reasoning deltas,
/// tool-call deltas, etc.).
fn process_responses_event(
    event: ResponsesStreamEvent,
) -> Result<Option<ResponseEvent>, ApiError> {
    match event.kind.as_str() {
        // Mirrors line 307
        "response.created" => Ok(Some(ResponseEvent::Created)),

        // Mirrors line 275
        "response.output_text.delta" => Ok(event.delta.map(ResponseEvent::OutputTextDelta)),

        // Mirrors line 358
        "response.completed" => match event.response {
            Some(resp_val) => match serde_json::from_value::<ResponseCompleted>(resp_val) {
                Ok(resp) => Ok(Some(ResponseEvent::Completed {
                    response_id: resp.id,
                })),
                Err(err) => Err(ApiError::Stream(format!(
                    "failed to parse ResponseCompleted: {err}"
                ))),
            },
            None => Ok(None),
        },

        // Mirrors line 312
        "response.failed" => Err(ApiError::Stream(
            "response.failed event received".to_string(),
        )),

        // Unknown / ignored kinds — codex maps these to ReasoningDelta / OutputItemDone
        // / ToolCallInputDelta etc.; for a minimal hello we drop them silently.
        _ => Ok(None),
    }
}

/// Mirrors: codex-rs/codex-api/src/sse/responses.rs:399  `process_sse`
///
/// Polls the upstream byte stream through `eventsource_stream::Eventsource`,
/// applies a per-poll idle timeout, decodes each event via
/// `process_responses_event`, and forwards typed events through `tx_event`.
/// Terminates once a `Completed` event is sent or any error occurs.
pub async fn process_sse(
    stream: ByteStream,
    tx_event: mpsc::Sender<Result<ResponseEvent, ApiError>>,
    idle_timeout: Duration,
) {
    let mut stream = stream.eventsource();

    loop {
        // Mirrors line 411: `tokio::time::timeout(idle_timeout, stream.next())`.
        // codex's default `idle_timeout` is 5 min (see
        // codex-rs/model-provider-info/src/lib.rs:26
        // `DEFAULT_STREAM_IDLE_TIMEOUT_MS = 300_000`).
        let response = timeout(idle_timeout, stream.next()).await;

        let sse = match response {
            // Mirrors line 416
            Ok(Some(Ok(sse))) => sse,
            // Mirrors line 417
            Ok(Some(Err(err))) => {
                let _ = tx_event
                    .send(Err(ApiError::Stream(err.to_string())))
                    .await;
                return;
            }
            // Mirrors line 422
            Ok(None) => {
                let _ = tx_event
                    .send(Err(ApiError::Stream(
                        "stream closed before response.completed".into(),
                    )))
                    .await;
                return;
            }
            // Mirrors line 429
            Err(_) => {
                let _ = tx_event
                    .send(Err(ApiError::Stream(
                        "idle timeout waiting for SSE".into(),
                    )))
                    .await;
                return;
            }
        };

        // Mirrors line 439 — JSON-decode the raw `data:` payload.
        let event: ResponsesStreamEvent = match serde_json::from_str(&sse.data) {
            Ok(event) => event,
            Err(err) => {
                let _ = tx_event
                    .send(Err(ApiError::Stream(format!(
                        "failed to parse SSE event: {err}"
                    ))))
                    .await;
                return;
            }
        };

        match process_responses_event(event) {
            Ok(Some(decoded)) => {
                let is_completed = matches!(decoded, ResponseEvent::Completed { .. });
                if tx_event.send(Ok(decoded)).await.is_err() {
                    return;
                }
                if is_completed {
                    return;
                }
            }
            Ok(None) => {}
            Err(err) => {
                let _ = tx_event.send(Err(err)).await;
                return;
            }
        }
    }
}

/// Extension beyond codex: SSE pump for `POST /v1/chat/completions`.
///
/// Chat-completions stream format differs from Responses API:
/// - Each `data:` payload is a `ChatStreamChunk` JSON object
/// - The terminal sentinel is the literal string `data: [DONE]`
/// - No `response.created` / `response.completed` semantic events; we
///   synthesize `ResponseEvent::Completed` when we see `[DONE]` or a
///   non-null `finish_reason`.
pub async fn process_chat_sse(
    stream: ByteStream,
    tx_event: mpsc::Sender<Result<ResponseEvent, ApiError>>,
    idle_timeout: Duration,
) {
    let mut stream = stream.eventsource();
    let mut last_id = String::new();

    loop {
        let response = timeout(idle_timeout, stream.next()).await;
        let sse = match response {
            Ok(Some(Ok(sse))) => sse,
            Ok(Some(Err(err))) => {
                let _ = tx_event
                    .send(Err(ApiError::Stream(err.to_string())))
                    .await;
                return;
            }
            Ok(None) => {
                let _ = tx_event
                    .send(Ok(ResponseEvent::Completed {
                        response_id: std::mem::take(&mut last_id),
                    }))
                    .await;
                return;
            }
            Err(_) => {
                let _ = tx_event
                    .send(Err(ApiError::Stream(
                        "idle timeout waiting for SSE".into(),
                    )))
                    .await;
                return;
            }
        };

        // Terminal sentinel — chat completions sends `data: [DONE]` instead of
        // a structured "completed" event.
        if sse.data.trim() == "[DONE]" {
            let _ = tx_event
                .send(Ok(ResponseEvent::Completed {
                    response_id: std::mem::take(&mut last_id),
                }))
                .await;
            return;
        }

        let chunk: ChatStreamChunk = match serde_json::from_str(&sse.data) {
            Ok(c) => c,
            Err(err) => {
                let _ = tx_event
                    .send(Err(ApiError::Stream(format!(
                        "failed to parse chat SSE chunk: {err}"
                    ))))
                    .await;
                return;
            }
        };
        if let Some(id) = chunk.id {
            last_id = id;
        }

        for choice in chunk.choices {
            if let Some(content) = choice.delta.content
                && !content.is_empty()
                && tx_event
                    .send(Ok(ResponseEvent::OutputTextDelta(content)))
                    .await
                    .is_err()
            {
                return;
            }
            if choice.finish_reason.is_some() {
                let _ = tx_event
                    .send(Ok(ResponseEvent::Completed {
                        response_id: std::mem::take(&mut last_id),
                    }))
                    .await;
                return;
            }
        }
    }
}
