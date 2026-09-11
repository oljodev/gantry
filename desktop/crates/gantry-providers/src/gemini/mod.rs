//! The Google Gemini client on the Interactions API (docs/plan/02 §4): stateless, function
//! calls with ids, thought signatures echoed on replay, streamed argument deltas on Gemini 3.
//!
//! The wire names follow Google's reference as read on 2026-09-07; the live conformance run
//! (`tests/live.rs`) is where a renamed event or field shows up first.

mod request;
pub mod stream;

pub use request::build_body;

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

pub const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

pub struct GeminiProvider {
    id: ProviderId,
    base_url: String,
    key: Option<SecretString>,
    http: reqwest::Client,
    models: RwLock<HashMap<String, ModelInfo>>,
}

impl std::fmt::Debug for GeminiProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeminiProvider")
            .field("id", &self.id)
            .field("base_url", &self.base_url)
            .field("has_key", &self.key.is_some())
            .finish()
    }
}

impl GeminiProvider {
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
            .ok_or_else(|| ProviderError::auth("No API key for Google"))?;
        let mut h = HeaderMap::new();
        http::put(&mut h, "x-goog-api-key", key.expose_secret());
        Ok(h)
    }

    fn url(&self, path: &str) -> String {
        http::join(&self.base_url, path)
    }
}

#[derive(Debug, Deserialize)]
struct ModelList {
    #[serde(default)]
    models: Vec<ModelRow>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelRow {
    name: String,
    display_name: Option<String>,
    input_token_limit: Option<u32>,
    output_token_limit: Option<u32>,
    #[serde(default)]
    supported_generation_methods: Vec<String>,
}

fn parse_models(json: &serde_json::Value) -> Result<Vec<ModelInfo>, ProviderError> {
    let list: ModelList = serde_json::from_value(json.clone())
        .map_err(|e| ProviderError::interrupted(format!("malformed model list: {e}")))?;
    let mut models: Vec<ModelInfo> = list
        .models
        .into_iter()
        .filter(|m| {
            m.supported_generation_methods.is_empty()
                || m.supported_generation_methods
                    .iter()
                    .any(|g| g == "generateContent")
        })
        .map(|m| {
            let id = m.name.strip_prefix("models/").unwrap_or(&m.name).to_owned();
            ModelInfo {
                display_name: m.display_name.unwrap_or_else(|| id.clone()),
                // Gemini's list carries no release date.
                created_at: None,
                context_window: m.input_token_limit,
                max_output: m.output_token_limit,
                pricing: None,
                capabilities: ModelCapabilities {
                    parallel_tools: true,
                    vision: true,
                    ..Default::default()
                },
                id,
            }
        })
        .filter(|m| m.id.starts_with("gemini"))
        .collect();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(models)
}

#[async_trait]
impl Provider for GeminiProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::Gemini
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
        let url = self.url("models?pageSize=1000");
        let json = crate::retry::with_retry(|| async {
            http::get_json(&self.http, &url, self.headers()?).await
        })
        .await?;
        let mut list = parse_models(&json)?;
        overrides::apply_all(ProviderKind::Gemini, self.id.as_str(), &mut list);
        *self.models.write().unwrap_or_else(|e| e.into_inner()) =
            list.iter().map(|m| (m.id.clone(), m.clone())).collect();
        Ok(list)
    }

    async fn check_key(&self) -> Result<KeyInfo, ProviderError> {
        http::get_json(&self.http, &self.url("models?pageSize=1"), self.headers()?).await?;
        Ok(KeyInfo::default())
    }

    async fn stream(&self, req: ChatRequest) -> Result<ChatStream, ProviderError> {
        let retries = req.retries;
        let headers = self.headers()?;
        let info = self.model_info(&req.model);
        let body = request::build_body(&req, info.as_ref());
        if log::log_enabled!(log::Level::Trace) {
            log::trace!("interactions request to Google: {body}");
        }
        let response = http::post_stream(
            &self.http,
            &self.url("interactions"),
            headers,
            &body,
            retries,
        )
        .await?;
        let events = sse_stream(response.bytes_stream(), FIRST_TOKEN_TIMEOUT, IDLE_TIMEOUT);
        Ok(stream::into_chat_stream(events))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_names_lose_their_prefix_and_keep_limits() {
        let json = serde_json::json!({ "models": [
            { "name": "models/gemini-2.5-flash", "displayName": "Gemini 2.5 Flash", "inputTokenLimit": 1048576,
              "outputTokenLimit": 65536, "supportedGenerationMethods": ["generateContent", "countTokens"] },
            { "name": "models/embedding-001", "displayName": "Embedding", "supportedGenerationMethods": ["embedContent"] },
            { "name": "models/imagen-4", "displayName": "Imagen", "supportedGenerationMethods": ["predict"] }
        ]});
        let models = parse_models(&json).unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "gemini-2.5-flash");
        assert_eq!(models[0].context_window, Some(1_048_576));
        assert_eq!(models[0].max_output, Some(65_536));
    }
}
