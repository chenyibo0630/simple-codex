//! Provider 元数据:`base_url` + `wire_api`。
//!
//! 对应 codex-rs/codex-api/src/provider.rs `Provider`。由调用方从
//! `core::config::ResolvedConfig` 构造,塞给 `ModelClient`。
//! `api_key` 不放这里(codex 的 auth 也与 provider 解耦)。

use simple_codex_core::config::WireApi;

#[derive(Debug, Clone)]
pub struct Provider {
    /// 已包含 API 版本前缀(如 `/v1`)。
    /// `ModelClient` 内部按 `wire_api` 拼 `/responses` 或 `/chat/completions`。
    pub base_url: String,

    /// 传输协议选择,决定 `ModelClient::stream` 走哪条分支。
    pub wire_api: WireApi,
}

impl Provider {
    pub fn new(base_url: String, wire_api: WireApi) -> Self {
        Self { base_url, wire_api }
    }
}
