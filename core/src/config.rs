//! Codex-style TOML configuration loader.
//!
//! Mirrors the shape of codex's `~/.codex/config.toml`:
//!   - top-level `model` and `model_provider`
//!   - a `[model_providers.<id>]` table per provider
//!   - per-provider `base_url`, `env_key`, `wire_api`, ...
//!
//! Source map:
//! - `ModelProviderInfo`   : codex-rs/model-provider-info/src/lib.rs:84
//! - `WireApi`             : codex-rs/model-provider-info/src/lib.rs:52
//! - Default OpenAI base   : codex-rs/core/src/config/config_tests.rs:7670
//! - Top-level `model` /
//!   `model_provider`      : codex-rs/core/src/config/mod.rs:535-536, :2066
//! - Provider resolution
//!   (find by id in map)   : codex-rs/core/src/config/mod.rs:2965-2973
//! - `env_key` → API key   : codex-rs/model-provider-info/src/lib.rs:273-281

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use crate::types::ApiError;

/// Top-level TOML schema.
/// Mirrors: codex-rs/core/src/config/config_toml.rs (the `ConfigToml` struct).
#[derive(Debug, Clone, Deserialize)]
pub struct ConfigToml {
    /// Active model slug (e.g. `gpt-5`, `minimax-m2.7`).
    pub model: String,

    /// Key into `model_providers` selecting which provider is active.
    /// Mirrors codex's top-level `model_provider = "openai"` field.
    pub model_provider: String,

    /// All declared providers, keyed by id. The id is the value used in
    /// `model_provider` above and in `--model-provider` overrides in codex.
    #[serde(default)]
    pub model_providers: HashMap<String, ModelProviderInfo>,
}

/// Mirrors: codex-rs/model-provider-info/src/lib.rs:84  `pub struct ModelProviderInfo`
/// Trimmed to the fields simple-codex actually consumes. Codex's full struct
/// also carries `experimental_bearer_token`, `auth`, `aws`, `query_params`,
/// `http_headers`, `env_http_headers`, retry/timeout knobs, etc.
#[derive(Debug, Clone, Deserialize)]
pub struct ModelProviderInfo {
    /// Display name. Codex defaults to "" when omitted.
    #[serde(default)]
    pub name: String,

    /// Provider base URL. Must already include the API version prefix
    /// (e.g. `/v1` or `/plan/v3`); simple-codex appends `/responses` or
    /// `/chat/completions` directly after it — same convention as codex
    /// (`Provider::url_for_path` in codex-rs/codex-api/src/provider.rs:53).
    pub base_url: Option<String>,

    /// Name of an env var to read for the API key. Codex's primary auth path
    /// (see `ModelProviderInfo::api_key` at
    /// codex-rs/model-provider-info/src/lib.rs:273).
    pub env_key: Option<String>,

    /// Direct key in TOML — **simple-codex extension**. Codex's nearest analogue
    /// is `experimental_bearer_token` (also marked "discouraged" there).
    /// Used as fallback when `env_key`'s env var is unset.
    pub api_key: Option<String>,

    /// Wire protocol. `Responses` mirrors codex's only supported choice;
    /// `Chat` is reintroduced by simple-codex (codex removed it in 7782).
    #[serde(default)]
    pub wire_api: WireApi,
}

/// Mirrors: codex-rs/model-provider-info/src/lib.rs:52  `pub enum WireApi`
/// (codex's current enum has just `Responses`; we extend with `Chat`).
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WireApi {
    #[default]
    Responses,
    Chat,
}

/// Resolved, runtime-ready view of the active provider with the API key
/// already pulled from `env_key` (or `api_key` fallback).
/// Mirrors the role of codex's `client_setup.api_provider` +
/// `client_setup.api_auth` pair (codex-rs/core/src/client.rs:785-794).
pub struct ResolvedConfig {
    pub model: String,
    pub provider_id: String,
    pub provider_display_name: String,
    pub base_url: String,
    pub api_key: String,
    pub wire_api: WireApi,
}

impl ConfigToml {
    /// Load `config.toml` from `path`.
    pub fn load(path: &Path) -> Result<Self, ApiError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| ApiError::Stream(format!("failed to read {}: {e}", path.display())))?;
        toml::from_str(&text)
            .map_err(|e| ApiError::Stream(format!("failed to parse {}: {e}", path.display())))
    }

    /// Resolve the active provider into a `ResolvedConfig`.
    ///
    /// Mirrors codex's resolution at codex-rs/core/src/config/mod.rs:2965-2973:
    /// look up `model_provider` in the providers map, error if missing.
    pub fn resolve(self) -> Result<ResolvedConfig, ApiError> {
        let ConfigToml {
            model,
            model_provider,
            mut model_providers,
        } = self;

        let provider = model_providers.remove(&model_provider).ok_or_else(|| {
            ApiError::Stream(format!(
                "Model provider `{model_provider}` not found under [model_providers.*]"
            ))
        })?;

        let base_url = provider.base_url.ok_or_else(|| {
            ApiError::Stream(format!(
                "Model provider `{model_provider}` is missing `base_url`"
            ))
        })?;

        // Mirrors codex's auth resolution priority
        // (`ModelProviderInfo::api_key` at
        // codex-rs/model-provider-info/src/lib.rs:273-281): try `env_key`
        // first, error if required-but-missing. simple-codex adds a
        // direct-key fallback for demo convenience.
        let api_key = if let Some(env_var) = provider.env_key.as_deref() {
            match std::env::var(env_var) {
                Ok(v) if !v.is_empty() => v,
                _ => provider.api_key.ok_or_else(|| {
                    ApiError::Stream(format!(
                        "Provider `{model_provider}`: env var `{env_var}` is unset \
                         and no `api_key` fallback is configured"
                    ))
                })?,
            }
        } else if let Some(direct) = provider.api_key {
            direct
        } else {
            return Err(ApiError::Stream(format!(
                "Provider `{model_provider}` has neither `env_key` nor `api_key`"
            )));
        };

        Ok(ResolvedConfig {
            model,
            provider_id: model_provider,
            provider_display_name: provider.name,
            base_url,
            api_key,
            wire_api: provider.wire_api,
        })
    }
}
