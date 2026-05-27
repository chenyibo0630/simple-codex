//! Driver — one-shot "hello" round-trip over SSE.
//!
//! Source map:
//! - The "build a `Prompt`, call `stream`, loop on `ResponseEvent`" pattern
//!   mirrors `try_run_sampling_request` at
//!   codex-rs/core/src/session/turn.rs:1712 (the `match event` block).
//! - `Prompt` construction mirrors codex-rs/core/src/client_common.rs:25
//!   and the model/input shape mirrors codex-rs/core/src/client.rs:746
//!   (`ResponsesApiRequest { .. }`).
//! - Config loading mirrors codex-rs/core/src/config/mod.rs:2960-2980
//!   (resolve active provider from `model_providers` map by id).
//!
//! Usage:
//!     # 1. Edit ./config.toml (model / model_provider / providers / keys).
//!     # 2. cargo run

mod client;
mod config;
mod sse;
mod types;

use std::io::Write;
use std::path::Path;

use client::ModelClient;
use config::{ConfigToml, WireApi};
use types::{ApiError, BaseInstructions, ContentItem, Prompt, ResponseEvent, ResponseItem};

#[tokio::main]
async fn main() -> Result<(), ApiError> {
    // Mirrors codex's config loading flow: read TOML → resolve provider id
    // against `model_providers.<id>` → pick API key from env (or direct fallback).
    let cfg = ConfigToml::load(Path::new("config.toml"))?.resolve()?;
    eprintln!(
        "[config] provider={} ({}) model={} wire_api={:?}",
        cfg.provider_id, cfg.provider_display_name, cfg.model, cfg.wire_api
    );

    let client = ModelClient::new(cfg.api_key, cfg.base_url, cfg.model);

    // Mirrors: codex-rs/core/src/client_common.rs:25  `pub struct Prompt`.
    // `..Default::default()` fills tools / parallel_tool_calls / personality /
    // output_schema* with codex's defaults (`impl Default for Prompt`).
    let prompt = Prompt {
        base_instructions: BaseInstructions {
            text: "You are a helpful assistant.".to_string(),
        },
        input: vec![ResponseItem::Message {
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: "你好".to_string(),
            }],
        }],
        ..Default::default()
    };

    // Dispatch on `wire_api`, matching codex's branch at
    // codex-rs/core/src/client.rs:1559 `match wire_api { WireApi::Responses => ... }`.
    // Codex now only has the Responses arm; the Chat arm is a simple-codex
    // extension (see `CHAT_WIRE_API_REMOVED_ERROR` in
    // codex-rs/model-provider-info/src/lib.rs:45).
    let mut rx = match cfg.wire_api {
        WireApi::Responses => client.stream(&prompt).await?,
        WireApi::Chat => client.stream_chat(&prompt).await?,
    };

    // Mirrors the consumer side of codex-rs/core/src/session/turn.rs:1785
    // `loop { stream.next().await ... }`.
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    while let Some(event) = rx.recv().await {
        match event {
            Ok(ResponseEvent::Created) => {
                eprintln!("[response.created]");
            }
            Ok(ResponseEvent::OutputTextDelta(delta)) => {
                let _ = stdout.write_all(delta.as_bytes());
                let _ = stdout.flush();
            }
            Ok(ResponseEvent::Completed { response_id }) => {
                let _ = stdout.write_all(b"\n");
                eprintln!("[response.completed id={response_id}]");
                break;
            }
            Err(err) => {
                eprintln!("[error] {err}");
                return Err(err);
            }
        }
    }
    Ok(())
}
