//! 二进制入口：加载 config，构造 `ModelClient` + `AppState`，启动 axum 服务。

use std::path::Path;
use std::sync::Arc;

use simple_codex_app_server::{AppState, serve};
use simple_codex_client::ModelClient;
use simple_codex_core::config::ConfigToml;
use simple_codex_core::types::ApiError;

#[tokio::main]
async fn main() -> Result<(), ApiError> {
    let cfg = ConfigToml::load(Path::new("config.toml"))?.resolve()?;
    eprintln!(
        "[config] provider={} ({}) model={} wire_api={:?}",
        cfg.provider_id, cfg.provider_display_name, cfg.model, cfg.wire_api
    );

    let client = ModelClient::new(cfg.api_key, cfg.base_url, cfg.model);
    let state = AppState {
        client: Arc::new(client),
        system_prompt: "You are a helpful assistant.".to_string(),
        wire_api: cfg.wire_api,
    };

    let addr: std::net::SocketAddr = "127.0.0.1:8080".parse().unwrap();
    eprintln!("[server] listening on http://{addr}");
    serve(addr, state)
        .await
        .map_err(|e| ApiError::Stream(format!("server error: {e}")))
}
