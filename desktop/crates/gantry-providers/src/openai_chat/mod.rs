//! The OpenAI-compatible Chat Completions client (docs/plan/02 §4): one implementation, one
//! [`CompatProfile`] per vendor. M1 ships the `openrouter` profile; `xai` and `custom` are
//! declared for M4.

mod key;
mod models;
mod profiles;
mod request;
pub mod stream;

use std::time::Duration;

use async_trait::async_trait;
use gantry_core::{ProviderId, ProviderKind};
use gantry_secrets::{ExposeSecret, SecretString};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

pub use profiles::{CompatProfile, KeyCheck, ModelsParser, ReasoningParam, ToolIdQuirk};

use crate::{
    error::ProviderError,
    provider::{ChatRequest, ChatStream, KeyInfo, ModelInfo, Provider},
    retry::{self, FIRST_TOKEN_TIMEOUT, IDLE_TIMEOUT},
    sse::sse_stream,
};

pub struct OpenAiChatProvider {
    id: ProviderId,
    profile: CompatProfile,
    key: Option<SecretString>,
    http: reqwest::Client,
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

/// The shared HTTP client: connect timeout only; per-request deadlines are applied per stream.
pub fn http_client(app_version: &str) -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(retry::CONNECT_TIMEOUT)
        .user_agent(format!("Gantry/{app_version}"))
        .build()
        .expect("a default reqwest client builds")
}

impl OpenAiChatProvider {
    #[must_use]
    pub fn new(
        id: ProviderId,
        profile: CompatProfile,
        key: Option<SecretString>,
        http: reqwest::Client,
    ) -> Self {
        Self {
            id,
            profile,
            key,
            http,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/{}", self.profile.base_url.trim_end_matches('/'), path)
    }

    fn headers(&self) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (name, value) in &self.profile.extra_headers {
            if let (Ok(n), Ok(v)) = (
                HeaderName::from_bytes(name.as_bytes()),
                HeaderValue::from_str(value),
            ) {
                h.insert(n, v);
            }
        }
        h
    }

    fn key(&self) -> Result<&SecretString, ProviderError> {
        self.key
            .as_ref()
            .ok_or_else(|| ProviderError::auth(format!("No API key for {}", self.profile.label)))
    }

    /// A GET with the key, mapped to a `ProviderError` on a non-2xx status.
    async fn get_json(&self, path: &str) -> Result<serde_json::Value, ProviderError> {
        let key = self.key()?;
        let response = self
            .http
            .get(self.url(path))
            .headers(self.headers())
            .bearer_auth(key.expose_secret())
            .timeout(Duration::from_secs(30))
            .send()
            .await?;
        let status = response.status();
        let retry_after = retry_after(response.headers());
        let body = response.text().await?;
        if !status.is_success() {
            return Err(ProviderError::from_status(
                status.as_u16(),
                &body,
                retry_after,
            ));
        }
        serde_json::from_str(&body)
            .map_err(|e| ProviderError::interrupted(format!("malformed JSON from {path}: {e}")))
    }
}

fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
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

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let json = retry::with_retry(|| self.get_json("models")).await?;
        models::parse(self.profile.models_parser, &json)
    }

    async fn check_key(&self) -> Result<KeyInfo, ProviderError> {
        match self.profile.key_check {
            KeyCheck::OpenRouterKey => {
                let json = self.get_json("key").await?;
                Ok(key::parse(&json))
            }
            KeyCheck::ListModels => {
                self.get_json("models").await?;
                Ok(KeyInfo::default())
            }
        }
    }

    async fn stream(&self, req: ChatRequest) -> Result<ChatStream, ProviderError> {
        let key = self.key()?;
        let body = request::build_body(&self.profile, &req);
        if log::log_enabled!(log::Level::Trace) {
            log::trace!("chat request to {}: {}", self.profile.label, body);
        }
        let url = self.url("chat/completions");
        let response = retry::with_retry(|| async {
            let response = self
                .http
                .post(&url)
                .headers(self.headers())
                .bearer_auth(key.expose_secret())
                .json(&body)
                .send()
                .await?;
            let status = response.status();
            if status.is_success() {
                return Ok(response);
            }
            let retry_after = retry_after(response.headers());
            let text = response.text().await.unwrap_or_default();
            Err(ProviderError::from_status(
                status.as_u16(),
                &text,
                retry_after,
            ))
        })
        .await?;
        let bytes = response.bytes_stream();
        let events = sse_stream(bytes, FIRST_TOKEN_TIMEOUT, IDLE_TIMEOUT);
        Ok(stream::into_chat_stream(events, self.profile.tool_id_quirk))
    }
}
