//! HTTP+SSE 服务库（mirrors codex-rs/app-server）。
//!
//! `serve(addr, state)` 是唯一对外入口；`AppState` 由调用方构造好后传入。

pub mod protocol;
pub mod routes;
pub mod state;
pub mod transport;

use std::net::SocketAddr;

use axum::Router;
use axum::routing::post;
use tokio::net::TcpListener;

pub use state::AppState;

pub async fn serve(addr: SocketAddr, state: AppState) -> std::io::Result<()> {
    let router = Router::new()
        .route("/v1/chat", post(routes::chat_handler))
        .with_state(state);
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, router).await
}
