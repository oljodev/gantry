//! The OpenAI Responses API client (docs/plan/02 §4): stateless (`store: false`), encrypted
//! reasoning replayed between tool rounds, function calls as items, the built-in web search.

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

pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

pub struct OpenAiResponsesProvider {
    id: ProviderId,
    base_url: String,
    key: Option<SecretString>,
    http: reqwest::Client,
    models: RwLock<HashMap<String, ModelInfo>>,
}

impl std::fmt::Debug for OpenAiResponsesProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiResponsesProvider")
            .field("id", &self.id)
            .field("base_url", &self.base_url)
            .field("has_key", &self.key.is_some())
            .finish()
    }
}

impl OpenAiResponsesProvider {
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
            .ok_or_else(|| ProviderError::auth("No API key for OpenAI"))?;
        let mut h = HeaderMap::new();
        http::put(
            &mut h,
            "authorization",
            &format!("Bearer {}", key.expose_secret()),
        );
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

#[derive(Debug, Deserialize)]
struct ModelRow {
    id: String,
    /// OpenAI dates every row in Unix seconds.
    created: Option<i64>,
}

/// Ids that are not chat models; the list mixes everything the account can reach.
const NOT_CHAT: &[&str] = &[
    "embedding",
    "tts",
    "whisper",
    "transcribe",
    "dall-e",
    "image",
    "moderation",
    "realtime",
    "audio",
    "babbage",
    "davinci",
    "computer-use",
    "search",
    "codex",
    "sora",
];

fn parse_models(json: &serde_json::Value) -> Result<Vec<ModelInfo>, ProviderError> {
    let list: ModelList = serde_json::from_value(json.clone())
        .map_err(|e| ProviderError::interrupted(format!("malformed model list: {e}")))?;
    let mut models: Vec<ModelInfo> = list
        .data
        .into_iter()
        .filter(|m| !NOT_CHAT.iter().any(|n| m.id.contains(n)))
        .map(|m| ModelInfo {
            display_name: m.id.clone(),
            created_at: m.created,
            context_window: None,
            max_output: None,
            pricing: None,
            capabilities: ModelCapabilities {
                parallel_tools: true,
                streams_tool_args: true,
                ..Default::default()
            },
            id: m.id,
        })
        .collect();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(models)
}

#[async_trait]
impl Provider for OpenAiResponsesProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::OpenAiResponses
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
        let url = self.url("models");
        let json = crate::retry::with_retry(|| async {
            http::get_json(&self.http, &url, self.headers()?).await
        })
        .await?;
        let mut list = parse_models(&json)?;
        overrides::apply_all(ProviderKind::OpenAiResponses, self.id.as_str(), &mut list);
        *self.models.write().unwrap_or_else(|e| e.into_inner()) =
            list.iter().map(|m| (m.id.clone(), m.clone())).collect();
        Ok(list)
    }

    async fn check_key(&self) -> Result<KeyInfo, ProviderError> {
        http::get_json(&self.http, &self.url("models"), self.headers()?).await?;
        Ok(KeyInfo::default())
    }

    async fn stream(&self, req: ChatRequest) -> Result<ChatStream, ProviderError> {
        let headers = self.headers()?;
        let info = self.model_info(&req.model);
        let body = request::build_body(&req, info.as_ref());
        if log::log_enabled!(log::Level::Trace) {
            log::trace!("responses request to OpenAI: {body}");
        }
        let response =
            http::post_stream(&self.http, &self.url("responses"), headers, &body).await?;
        let events = sse_stream(response.bytes_stream(), FIRST_TOKEN_TIMEOUT, IDLE_TIMEOUT);
        Ok(stream::into_chat_stream(events))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_model_list_keeps_chat_models_only() {
        let json = serde_json::json!({ "data": [
            { "id": "gpt-5-mini" }, { "id": "text-embedding-3-small" }, { "id": "gpt-4o-realtime-preview" },
            { "id": "o4-mini" }, { "id": "whisper-1" }
        ]});
        let ids: Vec<String> = parse_models(&json)
            .unwrap()
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(ids, ["gpt-5-mini", "o4-mini"]);
    }
}
