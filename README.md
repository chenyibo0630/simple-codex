# simple-codex

> 用于学习 [openai/codex](https://github.com/openai/codex) 内部架构的最小复刻。
> **当前版本 0.1** — 单轮 "你好" 流式问答,~500 行 Rust。

## 这是什么

把 codex(那个数十万行的 agent runtime)抽掉 95% 的功能后剩下的**协议骨架**:

- 装一个 `Prompt`(包含系统指令、对话历史、工具)
- 翻译成 OpenAI Responses API 或 Chat Completions API 的请求体
- POST 到 provider,接 SSE 字节流
- 用 `eventsource-stream` 解出 typed event,逐事件 dispatch
- 把模型吐出的 token 流式打到终端

代码文件、函数名、结构体字段都**严格对齐 codex 源码**,每个模块顶部都有 `Source map` 注释指明镜像了 codex 哪个文件的哪一行,可以直接对照阅读。

## v0.1 范围

✅ **复刻的部分**
- `Prompt` / `ResponseItem` / `ContentItem` / `ResponsesApiRequest` / `ResponseEvent` 等 wire 类型
- `process_responses_event` SSE 事件 dispatch(`response.created` / `output_text.delta` / `completed` / `failed`)
- `process_sse` 带 idle-timeout 的轮询主循环
- `build_responses_request` Prompt → API JSON 翻译
- `ModelClient::stream` 完整的 HTTP+SSE 出站路径
- `config.toml` codex 风格的 `[model_providers.<id>]` 配置
- WireApi 枚举 + 运行时分支

🟡 **codex 之外的扩展**(为了能跟国产 provider 跑通)
- `stream_chat` / `process_chat_sse` —— `/v1/chat/completions` 兼容路径
  > codex 在 PR #7782 已经删掉 chat 协议(`CHAT_WIRE_API_REMOVED_ERROR`),
  > 这里为适配 DeepSeek / Qwen / Tencent LKE / MiniMax 等只支持 chat 的 provider 加回来。
- `api_key` 字段直填进 TOML —— 仅为 demo 方便,codex 推荐 `env_key`

❌ **故意不做的部分**(看了 codex 你会知道这些有多大)
- WebSocket 传输(`responses_websocket.rs` 整套)
- 重试 + 降级(`stream_max_retries`、`force_http_fallback`)
- 断点续传(`previous_response_id` + `prepare_websocket_request`)
- 多轮历史累积(`ContextManager`)
- 工具执行循环(`ToolRouter` / `ToolCallRuntime`)
- Rollout 持久化(`~/.codex/sessions/*.jsonl`)
- App-server JSON-RPC 协议层
- MCP / 插件 / Sandbox / Approval / Guardian

## 文件地图

```
simple-codex/
├── Cargo.toml              依赖与 codex workspace 版本对齐
├── config.toml             实际配置(被 .gitignore 排除)
├── config.toml.example     模板,新克隆者从这里拷贝
└── src/
    ├── main.rs             驱动:加载配置 → 装 Prompt → 流式消费
    ├── config.rs           TOML 加载 + provider 解析 + WireApi 分支
    ├── types.rs            所有 wire 协议类型 + Prompt 结构
    ├── client.rs           ModelClient + stream / stream_chat
    └── sse.rs              process_responses_event + process_sse + chat 变体
```

## 参考

- codex 仓库: <https://github.com/openai/codex>
- 母版路径: `codex-rs/core` + `codex-rs/codex-api` + `codex-rs/protocol`
- OpenAI Responses API 文档: <https://platform.openai.com/docs/api-reference/responses>
- chat 协议被废止讨论: <https://github.com/openai/codex/discussions/7782>

## 协议 / License

代码仅用于学习,请勿用于生产。
不要把真 API key commit 到任何 git 仓库,即使是 private。
