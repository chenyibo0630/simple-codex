# simple-codex

> 用于学习 [openai/codex](https://github.com/openai/codex) 内部架构的最小复刻。
> **当前版本 0.2** — Cargo workspace + 三 crate 拆分,~600 行 Rust。

## 这是什么

把 codex(那个数十万行的 agent runtime)抽掉 95% 的功能后剩下的**协议骨架** + **架构骨架**:

- 装一个 `Prompt`(包含系统指令、对话历史、工具)
- 翻译成 OpenAI Responses API 或 Chat Completions API 的请求体
- POST 到 provider,接 SSE 字节流
- 用 `eventsource-stream` 解出 typed event,逐事件 dispatch
- 把模型吐出的 token 通过 HTTP+SSE 暴露成 `/v1/chat` 接口

代码文件、函数名、结构体字段、**crate 划分**都严格对齐 codex 源码,每个模块顶部都有 `Source map` 注释指明镜像了 codex 哪个文件的哪一行,可以直接对照阅读。

## Workspace 布局

```
simple-codex/
├── Cargo.toml              workspace 根:成员 + 共享依赖
├── config.toml             实际配置(被 .gitignore 排除)
├── config.toml.example     模板
│
├── core/                   simple-codex-core (lib)
│   └── src/
│       ├── lib.rs
│       ├── types.rs        Prompt / ResponseEvent / ApiError 等内部抽象
│       └── config.rs       config.toml schema + provider 解析
│       │                   ↑ 零 I/O 依赖,只用 serde + toml
│       │
│       │  ── 对应 codex-rs/protocol + codex-rs/model-provider-info
│
├── client/                 simple-codex-client (lib)
│   └── src/
│       ├── lib.rs          只 re-export `ModelClient`
│       ├── client.rs       ModelClient (reqwest) + stream / stream_chat
│       ├── sse.rs          process_sse + process_chat_sse
│       └── wire.rs         OpenAI wire-format DTO (pub(crate))
│       │
│       │  ── 对应 codex-rs/codex-client + codex-rs/codex-api
│
└── app-server/             simple-codex-app-server (lib + bin)
    └── src/
        ├── lib.rs          serve(addr, state) 入口
        ├── main.rs         binary:加载 config → 启动 axum
        ├── protocol.rs     UserTurnRequest / Op / EventMsg
        ├── routes.rs       POST /v1/chat handler
        ├── state.rs        AppState
        └── transport.rs    Op → Prompt → ResponseEvent → EventMsg SSE
        │
        │  ── 对应 codex-rs/app-server
```

依赖方向严格单向:`app-server → {client, core}`,`client → core`,`core` 零反向依赖。

## v0.2 范围

✅ **复刻的部分**
- `Prompt` / `ResponseItem` / `ContentItem` / `ResponsesApiRequest` / `ResponseEvent` 等 wire 类型
- `process_responses_event` SSE 事件 dispatch
- `process_sse` 带 idle-timeout 的轮询主循环
- `build_responses_request` Prompt → API JSON 翻译
- `ModelClient::stream` 完整的 HTTP+SSE 出站路径
- `config.toml` codex 风格的 `[model_providers.<id>]` 配置
- WireApi 枚举 + 运行时分支
- **app-server**:`POST /v1/chat` → SSE 流(对应 codex `app-server` crate)
- **Workspace 多 crate 拆分**:`core` / `client` / `app-server` 对齐 codex 主仓

🟡 **codex 之外的扩展**(为了能跟国产 provider 跑通)
- `stream_chat` / `process_chat_sse` —— `/v1/chat/completions` 兼容路径
  > codex 在 PR #7782 已经删掉 chat 协议(`CHAT_WIRE_API_REMOVED_ERROR`),
  > 这里为适配 DeepSeek / Qwen / Tencent LKE / MiniMax 等只支持 chat 的 provider 加回来。
- `api_key` 字段直填进 TOML —— 仅为 demo 方便,codex 推荐 `env_key`

❌ **故意不做的部分**
- WebSocket 传输(`responses_websocket.rs` 整套)
- 重试 + 降级(`stream_max_retries`、`force_http_fallback`)
- 断点续传(`previous_response_id` + `prepare_websocket_request`)
- 多轮历史累积(`ContextManager`)
- 工具执行循环(`ToolRouter` / `ToolCallRuntime`)
- Rollout 持久化(`~/.codex/sessions/*.jsonl`)
- App-server JSON-RPC 协议层
- MCP / 插件 / Sandbox / Approval / Guardian
- CLI 二进制(v0.1 有,v0.2 删除,聚焦 app-server)

## 跑起来

```bash
# 1) 拷模板填 key
cp config.toml.example config.toml
# 编辑 config.toml,填 provider 的 base_url + api_key

# 2) 启动服务
cargo run -p simple-codex-app-server
# [config] provider=minimax (MiniMax) model=... wire_api=Chat
# [server] listening on http://127.0.0.1:8080

# 3) 调用
curl -N -X POST http://127.0.0.1:8080/v1/chat \
  -H 'Content-Type: application/json' \
  -d '{"message":"你好"}'
```

返回 codex 风格的 SSE 事件:

```
event: task_started
data: {"type":"task_started"}

event: agent_message_delta
data: {"type":"agent_message_delta","delta":"你"}

event: agent_message
data: {"type":"agent_message","message":"你好"}

event: task_complete
data: {"type":"task_complete","response_id":"..."}
```

事件命名对齐 codex-rs/protocol `EventMsg::{TaskStarted, AgentMessageDelta, AgentMessage, TaskComplete, Error}`。

## 常用命令

```bash
cargo check --workspace                 # 全量检查
cargo run -p simple-codex-app-server    # 启动 :8080
cargo build -p simple-codex-client      # 单独编 client crate
```

## 参考

- codex 仓库:<https://github.com/openai/codex>
- 母版路径:`codex-rs/core` + `codex-rs/codex-client` + `codex-rs/codex-api` + `codex-rs/protocol` + `codex-rs/app-server`
- OpenAI Responses API 文档:<https://platform.openai.com/docs/api-reference/responses>
- chat 协议被废止讨论:<https://github.com/openai/codex/discussions/7782>

## 协议 / License

代码仅用于学习,请勿用于生产。
不要把真 API key commit 到任何 git 仓库,即使是 private。
