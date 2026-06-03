# simple-codex app-server 设计（最简版）

提供一个 HTTP+SSE 接口：客户端 `POST` 一个最简的 `{"message": "..."}`，服务端把它包装成内部 `Op::UserTurn { message }`，以 SSE 流式回推 `EventMsg::*`。

模块划分参考 codex：
- 数据契约抽到 `protocol`（对应 `codex-rs/protocol`）。
- HTTP 服务自成 `app_server` 子模块（对应 `codex-rs/app-server`），内部再拆 `transport / routes / state`，命名风格与 codex `exec-server/src/server/` 一致（`*_handler` 函数，路由路径如 `/v1/...`）。
- 为了最简化，**线缆格式扁平**（直接 `{"message": "..."}`），不像 codex 那样套一层 `Submission { id, op }`；`Op` 枚举仍保留作为内部表示，给未来扩展留口子。

---

## 1. 目录结构

```
src/
├── main.rs                # 极薄入口：加载 config + 解析 args + 分派到 cli 或 app_server
├── cli.rs                 # 新增：原 main.rs 的一次性 hello 流程搬到这里（mirrors codex-rs/cli）
├── client.rs              # 现有 ModelClient（保持不动）
├── config.rs              # 现有（保持不动）
├── sse.rs                 # 现有：上游 OpenAI SSE 解析器（保持不动）
├── types.rs               # 现有：Prompt / ResponseEvent / ApiError（保持不动）
└── app_server/            # 新增（mirrors codex-rs/app-server）
    ├── mod.rs             # serve(addr, state) 入口 + Router 装配
    ├── protocol.rs        # UserTurnRequest / Op / EventMsg（mirrors codex-rs/protocol 命名）
    ├── transport.rs       # axum SSE 适配 + ResponseEvent → EventMsg 转换
    ├── routes.rs          # chat_handler
    └── state.rs           # AppState { Arc<ModelClient> }
```

**老代码搬迁说明**（对照 codex 的 crate 切分）：

| 老代码块（`main.rs` 当前内容）                              | 搬迁目的地              | 对应 codex                                |
| ----------------------------------------------------------- | ----------------------- | ----------------------------------------- |
| `eprintln!("[config] ...")` + 构建 `Prompt` + 跑 SSE 循环   | `cli::run_hello(...)`   | `codex-rs/cli`（独立 CLI 入口）           |
| `ConfigToml::load + resolve + ModelClient::new` 这一段公共逻辑 | 留在 `main.rs`         | `codex-rs/cli/src/main.rs` 的引导段       |
| HTTP+SSE 服务（全新）                                       | `app_server::serve(..)` | `codex-rs/app-server`                     |

`main.rs` 退化为薄分派层，两条出口共用 config 加载和 ModelClient 构造。

> **codex 对照**：codex 真正的 `app-server` 走 WebSocket（`exec-server/src/server/transport.rs` 用 `WebSocketUpgrade`），我们只需 HTTP+SSE，因此 `transport.rs` 用 `axum::response::sse::Sse` 即可，其余模块切分照搬。

---

## 2. 协议层 `app_server/protocol.rs`

线缆请求格式被压扁到最简——`{"message": "..."}`，由 `UserTurnRequest` 接收；内部仍保留 `Op::UserTurn { message }` 作为业务表示，对齐 codex `Op` 命名风格，给未来扩展（`Op::Interrupt` 等）留扩展点。

响应去掉 codex 的 `Event { id, msg }` 关联壳——单请求-单 SSE 流，无需 `id` 关联，直接发 `EventMsg`。

```rust
// 线缆请求格式（最简）
#[derive(Debug, Deserialize)]
pub struct UserTurnRequest {
    pub message: String,
}

// 内部 Op 枚举：保留 codex 命名（mirrors codex-rs/protocol Op）
#[derive(Debug)]
pub enum Op {
    UserTurn { message: String },
}

impl From<UserTurnRequest> for Op {
    fn from(req: UserTurnRequest) -> Self {
        Op::UserTurn { message: req.message }
    }
}

// 响应事件载荷（mirrors codex-rs/protocol EventMsg 的命名风格）
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventMsg {
    TaskStarted,
    AgentMessageDelta { delta: String },
    AgentMessage { message: String },
    TaskComplete { response_id: String },
    Error { message: String },
}
```

> 命名对齐 codex：`Op/EventMsg`、`AgentMessage*`、`TaskStarted/TaskComplete`；只丢掉了 `Submission` 和 `Event` 外壳（线缆扁平化）。

---

## 3. 状态层 `app_server/state.rs`

```rust
#[derive(Clone)]
pub struct AppState {
    pub client: Arc<ModelClient>,
    pub system_prompt: String,   // 来自 config.toml 的 base_instructions
    pub wire_api: WireApi,       // Responses / Chat 分支
}
```

`Clone` 浅拷贝（内部 `Arc`），axum `State<AppState>` 直接用。

---

## 4. 路由层 `app_server/routes.rs`

只有一条路由：

| Method | Path            | Handler              |
| ------ | --------------- | -------------------- |
| POST   | `/v1/chat` | `chat_handler`  |

```rust
pub async fn chat_handler(
    State(state): State<AppState>,
    Json(req): Json<UserTurnRequest>,
) -> impl IntoResponse {
    transport::stream_chat(state, Op::from(req)).await
}
```

handler 只负责"接 JSON → 转换成内部 Op → 交给 transport"，保持薄。

---

## 5. 传输层 `app_server/transport.rs`

核心职责：
1. 把 `Op::UserTurn { message }` 翻译成现有 `Prompt`。
2. 调 `ModelClient.stream(&prompt)` 拿到 `mpsc::Receiver<Result<ResponseEvent, ApiError>>`。
3. 把每个 `ResponseEvent` 转成 `EventMsg`，封装为 axum `sse::Event`。

```rust
pub async fn stream_chat(state: AppState, op: Op) -> Sse<impl Stream<Item = ...>> {
    let prompt = build_prompt(&state.system_prompt, &op);
    let mut rx = match state.wire_api {
        WireApi::Responses => state.client.stream(&prompt).await,
        WireApi::Chat      => state.client.stream_chat(&prompt).await,
    }.unwrap_or_else(error_only_channel);

    let mut buffered = String::new();   // 拼出整段 AgentMessage
    let stream = async_stream::stream! {
        yield emit(EventMsg::TaskStarted);
        while let Some(event) = rx.recv().await {
            match event {
                Ok(ResponseEvent::Created) => {} // 已经发过 TaskStarted
                Ok(ResponseEvent::OutputTextDelta(delta)) => {
                    buffered.push_str(&delta);
                    yield emit(EventMsg::AgentMessageDelta { delta });
                }
                Ok(ResponseEvent::Completed { response_id }) => {
                    yield emit(EventMsg::AgentMessage { message: std::mem::take(&mut buffered) });
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
    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn emit(msg: EventMsg) -> Result<sse::Event, Infallible> {
    Ok(sse::Event::default()
        .event(event_name(&msg))         // 例如 "agent_message_delta"
        .json_data(&msg).unwrap())
}

// build_prompt: 把单条 message 包成 ResponseItem::Message { role: "user", content: [InputText] }
fn build_prompt(system: &str, op: &Op) -> Prompt {
    let Op::UserTurn { message } = op;
    Prompt {
        base_instructions: BaseInstructions { text: system.into() },
        input: vec![ResponseItem::Message {
            role: "user".into(),
            content: vec![ContentItem::InputText { text: message.clone() }],
        }],
        ..Default::default()
    }
}
```

**ResponseEvent → EventMsg 映射**：

| 上游 `ResponseEvent`         | 下游 `EventMsg`                        |
| ---------------------------- | -------------------------------------- |
| `Created`                    | `TaskStarted`（仅会话开始时发一次）    |
| `OutputTextDelta(s)`         | `AgentMessageDelta { delta: s }`       |
| `Completed { response_id }`  | `AgentMessage { message }` + `TaskComplete { response_id }` |
| `Err(ApiError)`              | `Error { message }`（并关闭流）        |

---

## 6. 入口 `app_server/mod.rs`

```rust
pub async fn serve(addr: SocketAddr, state: AppState) -> Result<(), std::io::Error> {
    let router = Router::new()
        .route("/v1/chat", post(routes::chat_handler))
        .with_state(state);
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, router).await
}
```

**`src/cli.rs`** — 原 main.rs 的一次性 hello 逻辑搬到这里：

```rust
use crate::client::ModelClient;
use crate::config::WireApi;
use crate::types::{ApiError, BaseInstructions, ContentItem, Prompt, ResponseEvent, ResponseItem};

pub async fn run_hello(
    client: ModelClient,
    system_prompt: String,
    wire_api: WireApi,
) -> Result<(), ApiError> {
    let prompt = Prompt {
        base_instructions: BaseInstructions { text: system_prompt },
        input: vec![ResponseItem::Message {
            role: "user".into(),
            content: vec![ContentItem::InputText { text: "你好".into() }],
        }],
        ..Default::default()
    };
    let mut rx = match wire_api {
        WireApi::Responses => client.stream(&prompt).await?,
        WireApi::Chat => client.stream_chat(&prompt).await?,
    };
    // ...原 main.rs 的 stdout 写入循环原样搬过来...
    Ok(())
}
```

**`src/main.rs`** — 退化为薄分派层，只保留 config 加载和分支选择：

```rust
mod app_server;
mod cli;
mod client;
mod config;
mod sse;
mod types;

#[tokio::main]
async fn main() -> Result<(), ApiError> {
    let cfg = ConfigToml::load(Path::new("config.toml"))?.resolve()?;
    eprintln!("[config] provider={} model={} wire_api={:?}", cfg.provider_id, cfg.model, cfg.wire_api);
    let client = ModelClient::new(cfg.api_key, cfg.base_url, cfg.model);
    let system_prompt = "You are a helpful assistant.".to_string();

    if std::env::args().any(|a| a == "--serve") {
        let state = AppState {
            client: Arc::new(client),
            system_prompt,
            wire_api: cfg.wire_api,
        };
        app_server::serve("127.0.0.1:8080".parse().unwrap(), state).await?;
    } else {
        cli::run_hello(client, system_prompt, cfg.wire_api).await?;
    }
    Ok(())
}
```

---

## 7. Cargo.toml 新增依赖

```toml
axum = { version = "0.7", default-features = false, features = ["http1", "json", "tokio", "macros"] }
tower = "0.5"
async-stream = "0.3"
```

`reqwest` / `tokio` / `serde` / `serde_json` 都已有。

---

## 8. 客户端调用示例

请求：
```bash
curl -N -X POST http://127.0.0.1:8080/v1/chat \
  -H 'Content-Type: application/json' \
  -d '{"message":"你好"}'
```

响应（SSE）：
```
event: task_started
data: {"type":"task_started"}

event: agent_message_delta
data: {"type":"agent_message_delta","delta":"你"}

event: agent_message_delta
data: {"type":"agent_message_delta","delta":"好"}

event: agent_message
data: {"type":"agent_message","message":"你好"}

event: task_complete
data: {"type":"task_complete","response_id":"resp_xxx"}
```

---

## 9. 与 codex 的偏离点

| 项                  | codex 现状                                  | 本设计                                |
| ------------------- | ------------------------------------------- | ------------------------------------- |
| 传输                | WebSocket（双向，多 submission 复用一连接） | 单次 HTTP+SSE，一个请求一个流         |
| 请求包装            | `Submission { id, op: Op::* }` 双层结构    | 扁平 `{"message": "..."}`，Op 仅内部 |
| 响应包装            | `Event { id, msg: EventMsg::* }` 关联 id   | 直接发 `EventMsg`，无 id              |
| Op 变体             | `UserInput`（含 environments / schema 等） | 只保留 `UserTurn { message }`         |
| EventMsg 变体       | 40+（工具调用、补丁、审批等）              | 5 个核心：start/delta/msg/done/err    |
| 多 turn 会话状态    | Session/Thread 持久化                       | 无状态：每次 POST 单回合              |

后续如要追平 codex：
- 把扁平请求换回 `Submission { id, op }` 包装、响应换回 `Event { id, msg }`，引入 id 关联（为多路复用做准备）。
- 把 HTTP POST 改成 WS upgrade（保留 `transport.rs` 接口不变）。
- 在 `state.rs` 加 `Sessions: DashMap<ThreadId, Conversation>`，让 `UserTurn` 带 `thread_id`。
- 扩 `EventMsg` 增加 `ExecCommandBegin/End`、`TokenCount` 等变体。

---

## 10. 落地顺序（实现 checklist）

1. 加依赖 → 建 `src/app_server/` 五个文件，先全部空 `pub fn`。
2. 实现 `protocol.rs`，跑 `cargo check`。
3. 实现 `state.rs` + `mod.rs` 的 `serve()`，挂一个 `200 OK` 的占位 handler 验证 axum 启动。
4. 实现 `transport.rs::stream_chat` 的非流式版本（一次性返回固定 SSE 串）跑通 curl。
5. 接上 `ModelClient.stream`，完成 `ResponseEvent → EventMsg` 真实转换。
6. 验证 Responses / Chat 两个 `wire_api` 分支都能流式输出。
