//! 线缆契约（mirrors codex-rs/protocol 的命名风格）。
//!
//! - `UserTurnRequest`：扁平 wire 格式 `{"message": "..."}`。
//! - `Op`：内部业务表示，保留 codex 的 `Op::UserTurn` 命名，给未来加
//!   `Op::Interrupt` 等变体留扩展点。
//! - `EventMsg`：响应事件，mirrors codex-rs/protocol `EventMsg::*` 的命名
//!   (`AgentMessage*` / `TaskStarted` / `TaskComplete` / `Error`)。

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct UserTurnRequest {
    pub message: String,
}

#[derive(Debug)]
pub enum Op {
    UserTurn { message: String },
}

impl From<UserTurnRequest> for Op {
    fn from(req: UserTurnRequest) -> Self {
        Op::UserTurn { message: req.message }
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventMsg {
    TaskStarted,
    AgentMessageDelta { delta: String },
    AgentMessage { message: String },
    TaskComplete { response_id: String },
    Error { message: String },
}

impl EventMsg {
    pub fn event_name(&self) -> &'static str {
        match self {
            EventMsg::TaskStarted => "task_started",
            EventMsg::AgentMessageDelta { .. } => "agent_message_delta",
            EventMsg::AgentMessage { .. } => "agent_message",
            EventMsg::TaskComplete { .. } => "task_complete",
            EventMsg::Error { .. } => "error",
        }
    }
}
