//! The Anthropic Messages API client (docs/plan/02 §4): thinking blocks replayed verbatim in
//! an append-only transcript, cache breakpoints, refusals as a stop reason, the web search
//! server tool as opaque blocks.

mod request;
pub mod stream;

pub(crate) use request::result_text;
pub use request::{adaptive_thinking, build_body, web_search_type};

use std::{collections::HashMap, sync::RwLock};

use async_trait::async_trait;
use gantry_core::{ProviderId, ProviderKind};
use gantry_secrets::{ExposeSecret, SecretString};
use reqwest::header::HeaderMap;
use serde::Deserialize;

use crate::{
    error::ProviderError,
    http, overrides,
    provider::{ChatRequest, ChatStream, KeyInfo, ModelCapabilities, ModelInfo, Provider},
    retry::{FIRST_TOKEN_TIMEOUT, IDLE_TIMEOUT},
    sse::sse_stream,
};

pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
pub const API_VERSION: &str = "2023-06-01";

pub struct AnthropicProvider {
    id: ProviderId,
    base_url: String,
    key: Option<SecretString>,
    http: reqwest::Client,
    models: RwLock<HashMap<String, ModelInfo>>,
}

impl std::fmt::Debug for AnthropicProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnthropicProvider")
            .field("id", &self.id)
            .field("base_url", &self.base_url)
            .field("has_key", &self.key.is_some())
            .finish()
    }
}

impl AnthropicProvider {
    #[must_use]
    pub fn new(
        id: ProviderId,
        base_url: Option<String>,
        key: Option<SecretString>,
        http: reqwest::Client,
        known_models: Vec<ModelInfo>,
    ) -> Self {
        Self {
            id,
            base_url: base_url
                .filter(|u| !u.trim().is_empty())
                .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned()),
            key,
            http,
            models: RwLock::new(
                known_models
                    .into_iter()
                    .map(|m| (m.id.clone(), m))
                    .collect(),
            ),
        }
    }

    fn headers(&self) -> Result<HeaderMap, ProviderError> {
        let key = self
            .key
            .as_ref()
            .ok_or_else(|| ProviderError::auth("No API key for Anthropic"))?;
        let mut h = HeaderMap::new();
        http::put(&mut h, "x-api-key", key.expose_secret());
        http::put(&mut h, "anthropic-version", API_VERSION);
        Ok(h)
    }

    fn url(&self, path: &str) -> String {
        http::join(&self.base_url, path)
    }
}

#[derive(Debug, Deserialize)]
struct ModelList {
    #[serde(default)]
    data: Vec<ModelRow>,
}

/// `GET /v1/models`: ids and display names, plus the token limits newer responses carry.
#[derive(Debug, Deserialize)]
struct ModelRow {
    id: String,
    display_name: Option<String>,
    max_input_tokens: Option<u32>,
    max_output_tokens: Option<u32>,
}

fn parse_models(json: &serde_json::Value) -> Result<Vec<ModelInfo>, ProviderError> {
    let list: ModelList = serde_json::from_value(json.clone())
        .map_err(|e| ProviderError::interrupted(format!("malformed model list: {e}")))?;
    let mut models: Vec<ModelInfo> = list
        .data
        .into_iter()
        .map(|m| ModelInfo {
            display_name: m.display_name.unwrap_or_else(|| m.id.clone()),
            // Anthropic dates its models as ISO strings; parsing them needs a date crate this
            // layer does not carry, so the age filter simply does not apply to them.
            created_at: None,
            context_window: m.max_input_tokens,
            max_output: m.max_output_tokens,
            pricing: None,
            capabilities: ModelCapabilities {
                parallel_tools: true,
                streams_tool_args: true,
                vision: true,
                ..Default::default()
            },
            id: m.id,
        })
        .collect();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(models)
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::Anthropic
    }

    fn has_key(&self) -> bool {
        self.key.is_some()
    }

    fn model_info(&self, model: &str) -> Option<ModelInfo> {
        self.models
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(model)
            .cloned()
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let url = self.url("v1/models?limit=1000");
        let json = crate::retry::with_retry(|| async {
            http::get_json(&self.http, &url, self.headers()?).await
        })
        .await?;
        let mut list = parse_models(&json)?;
        overrides::apply_all(ProviderKind::Anthropic, self.id.as_str(), &mut list);
        *self.models.write().unwrap_or_else(|e| e.into_inner()) =
            list.iter().map(|m| (m.id.clone(), m.clone())).collect();
        Ok(list)
    }

    async fn check_key(&self) -> Result<KeyInfo, ProviderError> {
        http::get_json(&self.http, &self.url("v1/models?limit=1"), self.headers()?).await?;
        Ok(KeyInfo::default())
    }

    async fn stream(&self, req: ChatRequest) -> Result<ChatStream, ProviderError> {
        let headers = self.headers()?;
        let info = self.model_info(&req.model);
        let body = request::build_body(&req, info.as_ref());
        if log::log_enabled!(log::Level::Trace) {
            log::trace!("messages request to Anthropic: {body}");
        }
        let response =
            http::post_stream(&self.http, &self.url("v1/messages"), headers, &body).await?;
        let events = sse_stream(response.bytes_stream(), FIRST_TOKEN_TIMEOUT, IDLE_TIMEOUT);
        Ok(stream::into_chat_stream(events))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_rows_keep_names_and_limits() {
        let json = serde_json::json!({ "data": [
            { "id": "claude-sonnet-4-5", "display_name": "Claude Sonnet 4.5", "type": "model", "created_at": "2025-09-29T00:00:00Z" },
            { "id": "claude-opus-5", "display_name": "Claude Opus 5", "max_input_tokens": 1000000, "max_output_tokens": 128000 }
        ]});
        let models = parse_models(&json).unwrap();
        assert_eq!(models[0].display_name, "Claude Opus 5");
        assert_eq!(models[0].context_window, Some(1_000_000));
        assert_eq!(models[1].id, "claude-sonnet-4-5");
        assert!(models[1].context_window.is_none());
    }
}
