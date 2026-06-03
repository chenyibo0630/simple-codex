//! HTTP 路由 handler。命名风格对齐 codex `exec-server/src/server/transport.rs`
//! 的 `*_handler` 约定。

use axum::Json;
use axum::extract::State;
use axum::response::IntoResponse;

use crate::protocol::{Op, UserTurnRequest};
use crate::state::AppState;
use crate::transport;

pub async fn chat_handler(
    State(state): State<AppState>,
    Json(req): Json<UserTurnRequest>,
) -> impl IntoResponse {
    transport::stream_chat(state, Op::from(req)).await
}
