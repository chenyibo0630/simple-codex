//! 共享状态。axum `State<AppState>` 直接传递，内部 `Arc` 浅拷贝。

use std::sync::Arc;

use simple_codex_client::ModelClient;
use simple_codex_core::config::WireApi;

#[derive(Clone)]
pub struct AppState {
    pub client: Arc<ModelClient>,
    pub system_prompt: String,
    pub wire_api: WireApi,
}
