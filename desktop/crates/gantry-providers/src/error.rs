//! Provider failures (docs/plan/02 §7), classified once so the agent and the UI can react
//! without knowing the vendor.

use std::time::Duration;

use gantry_core::{GantryError, ProviderErrorKind};

#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct ProviderError {
    pub kind: ProviderErrorKind,
    pub message: String,
    /// From a `Retry-After` header, when the provider sent one.
    pub retry_after: Option<Duration>,
    pub status: Option<u16>,
}

impl ProviderError {
    pub fn new(kind: ProviderErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            retry_after: None,
            status: None,
        }
    }

    pub fn auth(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::Auth, message)
    }

    pub fn network(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::Network, message)
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::InvalidRequest, message)
    }

    pub fn interrupted(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::StreamInterrupted, message)
    }

    /// Whether a fresh attempt could succeed (only meaningful before output started).
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        self.kind.is_retryable() && self.kind != ProviderErrorKind::StreamInterrupted
    }

    /// Classifies an HTTP failure. `body` is the response text, from which a provider's
    /// `error.message` is extracted when present.
    #[must_use]
    pub fn from_status(status: u16, body: &str, retry_after: Option<Duration>) -> Self {
        let message = error_message(body).unwrap_or_else(|| {
            if body.trim().is_empty() {
                format!("HTTP {status}")
            } else {
                format!("HTTP {status}: {}", truncate(body, 300))
            }
        });
        let kind = match status {
            401 | 403 => ProviderErrorKind::Auth,
            402 => ProviderErrorKind::InsufficientCredits,
            404 => ProviderErrorKind::NotFound,
            408 => ProviderErrorKind::Network,
            413 => ProviderErrorKind::ContextTooLong,
            429 => ProviderErrorKind::RateLimited,
            400 | 422 => {
                if message.to_ascii_lowercase().contains("context length")
                    || message.to_ascii_lowercase().contains("maximum context")
                {
                    ProviderErrorKind::ContextTooLong
                } else {
                    ProviderErrorKind::InvalidRequest
                }
            }
            502 | 503 | 529 => ProviderErrorKind::Overloaded,
            500..=599 => ProviderErrorKind::Overloaded,
            _ => ProviderErrorKind::Unknown,
        };
        Self {
            kind,
            message,
            retry_after,
            status: Some(status),
        }
    }
}

/// `{"error": {"message": "..."}}` or `{"error": "..."}`, as OpenAI-shaped APIs send them.
fn error_message(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let err = v.get("error")?;
    if let Some(s) = err.as_str() {
        return Some(s.to_owned());
    }
    let message = err.get("message")?.as_str()?.to_owned();
    let provider = err
        .get("metadata")
        .and_then(|m| m.get("provider_name"))
        .and_then(|p| p.as_str());
    Some(match provider {
        Some(p) => format!("{message} ({p})"),
        None => message,
    })
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_owned()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    }
}

impl From<reqwest::Error> for ProviderError {
    fn from(err: reqwest::Error) -> Self {
        let kind = if err.is_timeout() || err.is_connect() || err.is_request() {
            ProviderErrorKind::Network
        } else if err.is_body() || err.is_decode() {
            ProviderErrorKind::StreamInterrupted
        } else {
            ProviderErrorKind::Network
        };
        // reqwest's Display never includes headers, so the key cannot leak through here.
        Self::new(kind, err.without_url().to_string())
    }
}

impl From<ProviderError> for GantryError {
    fn from(err: ProviderError) -> Self {
        GantryError::Provider {
            kind: err.kind,
            message: err.message,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_openrouter_statuses() {
        let e = ProviderError::from_status(
            401,
            r#"{"error":{"code":401,"message":"No auth credentials found"}}"#,
            None,
        );
        assert_eq!(e.kind, ProviderErrorKind::Auth);
        assert_eq!(e.message, "No auth credentials found");
        assert_eq!(
            ProviderError::from_status(402, "", None).kind,
            ProviderErrorKind::InsufficientCredits
        );
        assert_eq!(
            ProviderError::from_status(429, "", None).kind,
            ProviderErrorKind::RateLimited
        );
        assert_eq!(
            ProviderError::from_status(503, "", None).kind,
            ProviderErrorKind::Overloaded
        );
        let e = ProviderError::from_status(
            400,
            r#"{"error":{"message":"This endpoint's maximum context length is 8192 tokens","metadata":{"provider_name":"X"}}}"#,
            None,
        );
        assert_eq!(e.kind, ProviderErrorKind::ContextTooLong);
        assert!(e.message.ends_with("(X)"));
    }
}
