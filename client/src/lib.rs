//! HTTP+SSE 模型传输层（mirrors codex-rs/codex-client）。
//!
//! - `wire`：OpenAI / Chat-Completions 的 wire-format DTO（不外泄）
//! - `sse`：SSE 解析 → `core::ResponseEvent`（内部）
//! - `client`：`ModelClient` reqwest 实现，对外唯一入口

mod client;
mod sse;
mod wire;

pub use client::ModelClient;
