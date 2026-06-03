//! 把内部 `Op` → `Prompt` → 上游 `ResponseEvent` 流 → 下游 `EventMsg` SSE 流。
//!
//! 仅一个对外函数 `stream_chat`，由 `routes::chat_handler` 调用。

use std::convert::Infallible;

use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use futures::stream::Stream;

use simple_codex_core::config::WireApi;
use simple_codex_core::types::{BaseInstructions, ContentItem, Prompt, ResponseEvent, ResponseItem};

use crate::protocol::{EventMsg, Op};
use crate::state::AppState;

pub async fn stream_chat(
    state: AppState,
    op: Op,
) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    let prompt = build_prompt(&state.system_prompt, &op);
    let stream_result = match state.wire_api {
        WireApi::Responses => state.client.stream(&prompt).await,
        WireApi::Chat => state.client.stream_chat(&prompt).await,
    };

    let s = async_stream::stream! {
        yield emit(EventMsg::TaskStarted);

        let mut rx = match stream_result {
            Ok(rx) => rx,
            Err(err) => {
                yield emit(EventMsg::Error { message: err.to_string() });
                return;
            }
        };

        let mut buffered = String::new();
        while let Some(event) = rx.recv().await {
            match event {
                Ok(ResponseEvent::Created) => {
                    // 已经发过 TaskStarted，这里跳过。
                }
                Ok(ResponseEvent::OutputTextDelta(delta)) => {
                    buffered.push_str(&delta);
                    yield emit(EventMsg::AgentMessageDelta { delta });
                }
                Ok(ResponseEvent::Completed { response_id }) => {
                    yield emit(EventMsg::AgentMessage {
                        message: std::mem::take(&mut buffered),
                    });
                    yield emit(EventMsg::TaskComplete { response_id });
                    break;
                }
                Err(err) => {
                    yield emit(EventMsg::Error { message: err.to_string() });
                    break;
                }
            }
        }
    };

    Sse::new(s).keep_alive(KeepAlive::default())
}

fn emit(msg: EventMsg) -> Result<SseEvent, Infallible> {
    let name = msg.event_name();
    // `.json_data` 失败仅当 serialize 失败；EventMsg 全是 String 字段，不会失败。
    let evt = SseEvent::default().event(name).json_data(&msg).unwrap();
    Ok(evt)
}

fn build_prompt(system: &str, op: &Op) -> Prompt {
    let Op::UserTurn { message } = op;
    Prompt {
        base_instructions: BaseInstructions { text: system.to_string() },
        input: vec![ResponseItem::Message {
            role: "user".to_string(),
            content: vec![ContentItem::InputText { text: message.clone() }],
        }],
        ..Default::default()
    }
}
