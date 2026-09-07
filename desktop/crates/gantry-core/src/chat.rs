//! Chat and turn DTOs as the frontend sees them (docs/plan/01 §4). In M1 chats live in memory;
//! M2 persists them without changing these shapes.

use serde::{Deserialize, Serialize};

use crate::{
    ids::{ChatId, ProjectId, TurnId},
    message::{Message, StopReason, Usage},
    settings::{Mode, ModelRef, ReasoningEffort},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum TurnStatus {
    Running,
    Completed,
    Cancelled,
    Failed,
}

impl TurnStatus {
    #[must_use]
    pub fn is_final(self) -> bool {
        !matches!(self, TurnStatus::Running)
    }
}

/// A sidebar row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ChatSummary {
    pub id: ChatId,
    pub title: String,
    pub pinned: bool,
    pub project_id: Option<ProjectId>,
    #[specta(type = specta_typescript::Number)]
    pub created_at: i64,
    #[specta(type = specta_typescript::Number)]
    pub last_message_at: i64,
    /// The turn currently streaming, if any.
    pub active_turn: Option<TurnId>,
}

/// One user message and the assistant's reply to it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct TurnDto {
    pub id: TurnId,
    pub status: TurnStatus,
    pub model: ModelRef,
    pub user: Message,
    /// Absent while the turn is running; the live parts come through the channel.
    pub assistant: Option<Message>,
    pub usage: Option<Usage>,
    pub stop_reason: Option<StopReason>,
    pub error: Option<String>,
    #[specta(type = specta_typescript::Number)]
    pub started_at: i64,
    #[specta(type = Option<specta_typescript::Number>)]
    pub ended_at: Option<i64>,
}

/// Everything the chat view needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct ChatDetail {
    pub id: ChatId,
    pub title: String,
    pub pinned: bool,
    pub project_id: Option<ProjectId>,
    #[specta(type = specta_typescript::Number)]
    pub created_at: i64,
    #[specta(type = specta_typescript::Number)]
    pub last_message_at: i64,
    pub model: ModelRef,
    pub mode: Mode,
    pub guard: bool,
    pub effort: ReasoningEffort,
    pub active_turn: Option<TurnId>,
    pub turns: Vec<TurnDto>,
}
