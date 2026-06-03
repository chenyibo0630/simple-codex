//! HTTP+SSE 模型传输层(mirrors codex-rs/codex-client + codex-api)。
//!
//! - `wire`:OpenAI / Chat-Completions 的 wire-format DTO(不外泄)
//! - `sse`:SSE 解析 → `core::ResponseEvent`(内部)
//! - `provider`:`Provider` 元数据(base_url + wire_api)
//! - `client`:`ModelClient` reqwest 实现,对外唯一入口

mod client;
mod provider;
mod sse;
mod wire;

pub use client::ModelClient;
pub use provider::Provider;
