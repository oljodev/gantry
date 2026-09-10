//! The OpenAI-compatible Chat Completions client (docs/plan/02 §4): one implementation, one
//! [`CompatProfile`] per vendor (`openrouter`, `xai`, `custom`).

mod key;
pub mod media;
mod models;
mod profiles;
mod request;
pub mod stream;

use std::{collections::HashMap, sync::RwLock};

use async_trait::async_trait;
use gantry_core::{ProviderId, ProviderKind};
use gantry_secrets::{ExposeSecret, SecretString};
use reqwest::header::HeaderMap;

pub use profiles::{
    CompatProfile, KeyCheck, ModelsParser, ReasoningParam, ToolIdQuirk, WebSearchParam,
};
pub use request::build_body;

pub use crate::http::http_client;
use crate::{
    error::ProviderError,
    http, overrides,
    provider::{ChatRequest, ChatStream, KeyInfo, ModelInfo, Provider},
    retry::{FIRST_TOKEN_TIMEOUT, IDLE_TIMEOUT},
    sse::sse_stream,
};

pub struct OpenAiChatProvider {
    id: ProviderId,
    profile: CompatProfile,
    key: Option<SecretString>,
    http: reqwest::Client,
    /// What the last model list said, by model id; seeded from the catalog cache.
    models: RwLock<HashMap<String, ModelInfo>>,
}

impl std::fmt::Debug for OpenAiChatProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiChatProvider")
            .field("id", &self.id)
            .field("base_url", &self.profile.base_url)
            .field("has_key", &self.key.is_some())
            .finish()
    }
}

impl OpenAiChatProvider {
    #[must_use]
    pub fn new(
        id: ProviderId,
        profile: CompatProfile,
        key: Option<SecretString>,
        http: reqwest::Client,
        known_models: Vec<ModelInfo>,
    ) -> Self {
        Self {
            id,
            profile,
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

    fn url(&self, path: &str) -> String {
        http::join(&self.profile.base_url, path)
    }

    /// The vendor's extra headers plus the bearer key. A custom endpoint may have no key
    /// (a local server); every hosted profile requires one.
    fn headers(&self) -> Result<HeaderMap, ProviderError> {
        let mut h = HeaderMap::new();
        for (name, value) in &self.profile.extra_headers {
            http::put(&mut h, name, value);
        }
        match &self.key {
            Some(key) => http::put(
                &mut h,
                "authorization",
                &format!("Bearer {}", key.expose_secret()),
            ),
            None if self.profile.key_optional => {}
            None => {
                return Err(ProviderError::auth(format!(
                    "No API key for {}",
                    self.profile.label
                )));
            }
        }
        Ok(h)
    }

    async fn get_json(&self, path: &str) -> Result<serde_json::Value, ProviderError> {
        http::get_json(&self.http, &self.url(path), self.headers()?).await
    }
}

#[async_trait]
impl Provider for OpenAiChatProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::OpenAiChat
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
        let json = crate::retry::with_retry(|| self.get_json(self.profile.models_path)).await?;
        let mut list = models::parse(self.profile.models_parser, &json)?;
        // The kinds the plain list leaves out. A category that fails is logged and skipped:
        // losing the video models is a smaller loss than losing the whole list with them.
        for category in self.profile.model_categories {
            let path = format!("{}?output_modality={category}", self.profile.models_path);
            match self.get_json(&path).await {
                Ok(json) => match models::parse(self.profile.models_parser, &json) {
                    Ok(extra) => {
                        let known: std::collections::HashSet<String> =
                            list.iter().map(|m| m.id.clone()).collect();
                        list.extend(extra.into_iter().filter(|m| !known.contains(&m.id)));
                    }
                    Err(err) => log::warn!("the {category} model list did not parse: {err}"),
                },
                Err(err) => log::warn!("the {category} model list could not be read: {err}"),
            }
        }
        list.sort_by(|a, b| a.id.cmp(&b.id));
        overrides::apply_all(ProviderKind::OpenAiChat, self.id.as_str(), &mut list);
        *self.models.write().unwrap_or_else(|e| e.into_inner()) =
            list.iter().map(|m| (m.id.clone(), m.clone())).collect();
        Ok(list)
    }

    async fn check_key(&self) -> Result<KeyInfo, ProviderError> {
        match self.profile.key_check {
            KeyCheck::OpenRouterKey => {
                let json = self.get_json("key").await?;
                Ok(key::parse(&json))
            }
            KeyCheck::ListModels => {
                self.get_json(self.profile.models_path).await?;
                Ok(KeyInfo::default())
            }
        }
    }

    async fn stream(&self, req: ChatRequest) -> Result<ChatStream, ProviderError> {
        let headers = self.headers()?;
        let info = self.model_info(&req.model);
        // A model that makes a picture, a voice or a clip answers somewhere else entirely.
        if self.profile.media_endpoints {
            match media::route(info.as_ref()) {
                Some(media::MediaRoute::Image) => {
                    return media::image(&self.http, &self.profile.base_url, headers, &req).await;
                }
                Some(media::MediaRoute::Speech) => {
                    return media::speech(
                        &self.http,
                        &self.profile.base_url,
                        headers,
                        &req,
                        info.as_ref(),
                    )
                    .await;
                }
                Some(media::MediaRoute::Video) => {
                    return Ok(media::video(
                        &self.http,
                        &self.profile.base_url,
                        headers,
                        &req,
                    ));
                }
                None => {}
            }
        }
        let body = request::build_body(&self.profile, &req, info.as_ref());
        if log::log_enabled!(log::Level::Trace) {
            log::trace!("chat request to {}: {}", self.profile.label, body);
        }
        let url = self.url("chat/completions");
        let response = http::post_stream(&self.http, &url, headers, &body).await?;
        let bytes = response.bytes_stream();
        let events = sse_stream(bytes, FIRST_TOKEN_TIMEOUT, IDLE_TIMEOUT);
        Ok(stream::into_chat_stream(events, self.profile.tool_id_quirk))
    }
}
