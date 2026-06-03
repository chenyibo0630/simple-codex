//! SSE pipeline.
//!
//! Source map:
//! - `process_responses_event` : codex-rs/codex-api/src/sse/responses.rs:263
//! - `process_sse`             : codex-rs/codex-api/src/sse/responses.rs:399

use std::time::Duration;

use eventsource_stream::Eventsource;
use futures::StreamExt;
use tokio::sync::mpsc;
use tokio::time::timeout;

use simple_codex_core::types::{ApiError, ResponseEvent};

use crate::wire::{ByteStream, ChatStreamChunk, ResponseCompleted, ResponsesStreamEvent};

/// Mirrors: codex-rs/codex-api/src/sse/responses.rs:263  `process_responses_event`
fn process_responses_event(
    event: ResponsesStreamEvent,
) -> Result<Option<ResponseEvent>, ApiError> {
    match event.kind.as_str() {
        "response.created" => Ok(Some(ResponseEvent::Created)),

        "response.output_text.delta" => Ok(event.delta.map(ResponseEvent::OutputTextDelta)),

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

        "response.failed" => Err(ApiError::Stream(
            "response.failed event received".to_string(),
        )),

        _ => Ok(None),
    }
}

/// Mirrors: codex-rs/codex-api/src/sse/responses.rs:399  `process_sse`
pub(crate) async fn process_sse(
    stream: ByteStream,
    tx_event: mpsc::Sender<Result<ResponseEvent, ApiError>>,
    idle_timeout: Duration,
) {
    let mut stream = stream.eventsource();

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
                    .send(Err(ApiError::Stream(
                        "stream closed before response.completed".into(),
                    )))
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
pub(crate) async fn process_chat_sse(
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
