//! Artifacts (docs/plan/13): versioned content shown beside the chat. The type is a string
//! validated against the renderer registry in `gantry-agent`; the panel renders whatever the
//! registry says.

use serde::{Deserialize, Serialize};

use crate::ids::{ArtifactId, ChatId, MessageId, ProjectId};

/// Who made a version (13 §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum VersionSource {
    ModelCreate,
    ModelUpdate,
    ModelEdit,
    UserEdit,
    UserRestore,
}

impl VersionSource {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            VersionSource::ModelCreate => "model_create",
            VersionSource::ModelUpdate => "model_update",
            VersionSource::ModelEdit => "model_edit",
            VersionSource::UserEdit => "user_edit",
            VersionSource::UserRestore => "user_restore",
        }
    }

    #[must_use]
    pub fn is_user(self) -> bool {
        matches!(self, VersionSource::UserEdit | VersionSource::UserRestore)
    }
}

/// The `artifacts` row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ArtifactDto {
    pub id: ArtifactId,
    pub chat_id: ChatId,
    pub project_id: Option<ProjectId>,
    /// `markdown`, `code`, `html`, `svg`, `mermaid`, `react`.
    #[serde(rename = "type")]
    pub artifact_type: String,
    pub title: String,
    pub language: Option<String>,
    pub summary: Option<String>,
    pub current_version: u32,
    pub created_by_message_id: Option<MessageId>,
    #[specta(type = specta_typescript::Number)]
    pub created_at: i64,
    #[specta(type = specta_typescript::Number)]
    pub updated_at: i64,
}

/// One `artifact_versions` row without its content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ArtifactVersionDto {
    pub artifact_id: ArtifactId,
    pub version: u32,
    pub source: VersionSource,
    pub tool_call_id: Option<String>,
    pub message_id: Option<MessageId>,
    pub note: Option<String>,
    #[specta(type = specta_typescript::Number)]
    pub size: u64,
    #[specta(type = specta_typescript::Number)]
    pub created_at: i64,
}

/// An artifact with the content of one version and its history, as the panel reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ArtifactContent {
    pub artifact: ArtifactDto,
    pub version: u32,
    pub content: String,
    pub versions: Vec<ArtifactVersionDto>,
}

/// What the renderer reported for one version (13 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RenderStatus {
    Ok,
    Error,
    /// The renderer had not reported within the cap.
    Pending,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct RenderError {
    /// `compile` or `runtime`.
    pub phase: String,
    pub message: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct RenderReport {
    pub status: RenderStatus,
    #[serde(default)]
    pub errors: Vec<RenderError>,
}

impl RenderReport {
    #[must_use]
    pub fn ok() -> Self {
        Self {
            status: RenderStatus::Ok,
            errors: Vec::new(),
        }
    }

    #[must_use]
    pub fn pending() -> Self {
        Self {
            status: RenderStatus::Pending,
            errors: Vec::new(),
        }
    }
}

/// Limits every write path enforces (13 §2).
pub const TITLE_MAX_CHARS: usize = 120;
pub const SUMMARY_MAX_CHARS: usize = 200;
pub const CONTENT_MAX_BYTES: usize = 1024 * 1024;
