//! simple-codex 共享类型 + 配置（纯定义，无 I/O 依赖）。
//!
//! 对应 codex-rs/protocol + codex-rs/model-provider-info（裁剪版）。
//! - `types`：`Prompt`、`ResponseEvent`、`ApiError` 等内部抽象
//! - `config`：`config.toml` schema + 解析
//!
//! 不依赖任何 HTTP 客户端或服务端框架。
//! HTTP+SSE 传输实现见 `simple-codex-client`。

pub mod config;
pub mod types;
