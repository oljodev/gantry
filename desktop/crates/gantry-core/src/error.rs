//! The crate-level error and its serialisable projection for the IPC boundary.

use serde::{Deserialize, Serialize};

/// The class of a provider failure (docs/plan/02 §7), shared by the client layer and the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ProviderErrorKind {
    /// The key is missing, invalid or disabled.
    Auth,
    /// The account has no credit left.
    InsufficientCredits,
    RateLimited,
    Overloaded,
    InvalidRequest,
    ContextTooLong,
    /// The model id is unknown to the provider.
    NotFound,
    Network,
    /// The stream ended before the message did.
    StreamInterrupted,
    /// The model declined to answer.
    Refused,
    Unknown,
}

impl ProviderErrorKind {
    /// Whether a retry without changes could succeed.
    #[must_use]
    pub fn is_retryable(self) -> bool {
        matches!(
            self,
            Self::RateLimited | Self::Overloaded | Self::Network | Self::StreamInterrupted
        )
    }
}

/// Errors raised anywhere in the backend. Variants grow with the milestones.
#[derive(Debug, thiserror::Error)]
pub enum GantryError {
    #[error("{0}")]
    Internal(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("{message}")]
    Provider {
        kind: ProviderErrorKind,
        message: String,
    },
    #[error("secret store: {0}")]
    Secrets(String),
    #[error("store: {0}")]
    Store(String),
}

impl GantryError {
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::InvalidInput(message.into())
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }
}

/// What the frontend receives when a command fails. Never carries secrets or paths the
/// user did not choose. Rendered by the UI as an inline error row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ErrorDto {
    Internal {
        message: String,
    },
    InvalidInput {
        message: String,
    },
    NotFound {
        message: String,
    },
    Io {
        message: String,
    },
    Provider {
        provider_kind: ProviderErrorKind,
        message: String,
    },
    Secrets {
        message: String,
    },
    Store {
        message: String,
    },
}

impl From<GantryError> for ErrorDto {
    fn from(err: GantryError) -> Self {
        match err {
            GantryError::Internal(message) => Self::Internal { message },
            GantryError::InvalidInput(message) => Self::InvalidInput { message },
            GantryError::NotFound(message) => Self::NotFound { message },
            GantryError::Io(e) => Self::Io {
                message: e.to_string(),
            },
            GantryError::Provider { kind, message } => Self::Provider {
                provider_kind: kind,
                message,
            },
            GantryError::Secrets(message) => Self::Secrets { message },
            GantryError::Store(message) => Self::Store { message },
        }
    }
}
