//! The crate-level error and its serialisable projection for the IPC boundary.

use serde::{Deserialize, Serialize};

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
}

impl GantryError {
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }
}

/// What the frontend receives when a command fails. Never carries secrets or paths the
/// user did not choose. Rendered by the UI as an inline error row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ErrorDto {
    Internal { message: String },
    InvalidInput { message: String },
    NotFound { message: String },
    Io { message: String },
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
        }
    }
}
