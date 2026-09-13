//! Chat and turn DTOs as the frontend sees them (docs/plan/01 §4). In M1 chats live in memory;
//! M2 persists them without changing these shapes.

use serde::{Deserialize, Serialize};

use crate::{
    ids::{ChatId, MessageId, ProjectId, TurnId},
    message::{Message, StopReason, Usage},
    settings::{Mode, ModelRef, ReasoningEffort},
    tool::ToolCallDto,
};

/// Prompt layer 6 is capped here (10 §2). The same 4,000 characters as the global layer: a
/// chat's instructions are the most specific layer and the most likely to be written in a hurry,
/// and the budget they spend is paid on every turn of that one conversation.
pub const CHAT_INSTRUCTIONS_MAX_CHARS: usize = 4000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum TurnStatus {
    Running,
    Completed,
    Cancelled,
    Failed,
    /// The app was closed while the turn ran (found at the next start).
    Interrupted,
}

impl TurnStatus {
    #[must_use]
    pub fn is_final(self) -> bool {
        !matches!(self, TurnStatus::Running)
    }
}

/// Which of the app's two surfaces a session belongs to (docs/plan/16 §2, C3). It is chosen at
/// creation and never changes: a session whose tools changed halfway would have a transcript
/// that cannot be explained.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    /// Conversation, documents, research, connectors, artifacts.
    #[default]
    Chat,
    /// Working inside a folder on this machine.
    Code,
}

impl Surface {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Surface::Chat => "chat",
            Surface::Code => "code",
        }
    }

    /// Whether a session on this surface must have a folder before its first turn (16 C5).
    #[must_use]
    pub fn needs_folder(self) -> bool {
        matches!(self, Surface::Code)
    }
}

/// The user's verdict on an assistant reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Feedback {
    Good,
    Bad,
}

/// A sidebar row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ChatSummary {
    pub id: ChatId,
    pub surface: Surface,
    /// The folders this session may reach; the sidebar names the first one under a code
    /// session's title (16 §5).
    pub roots: Vec<String>,
    pub title: String,
    pub pinned: bool,
    pub archived: bool,
    pub project_id: Option<ProjectId>,
    #[specta(type = specta_typescript::Number)]
    pub created_at: i64,
    #[specta(type = specta_typescript::Number)]
    pub last_message_at: i64,
    /// The turn currently streaming, if any.
    pub active_turn: Option<TurnId>,
    /// An incognito session (15 A21): its own window, no memory either way, and gone when the
    /// window closes. Never in a list the user browses, so this is only ever `true` for the
    /// one chat an incognito window is showing.
    pub incognito: bool,
}

/// One user message and everything the assistant did in reply.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct TurnDto {
    pub id: TurnId,
    pub status: TurnStatus,
    pub model: ModelRef,
    pub user: Message,
    /// The assistant and tool messages of the turn in order: one assistant message per model
    /// round, a tool message after each round that called tools. Empty while the turn runs;
    /// the live messages come through the channel.
    pub messages: Vec<Message>,
    /// Every tool call of the turn, in the order the model made them.
    pub tool_calls: Vec<ToolCallDto>,
    /// Provider and loop notices shown inline (05 §1): a round cap, a dropped block, a retry.
    pub notices: Vec<String>,
    pub usage: Option<Usage>,
    pub stop_reason: Option<StopReason>,
    pub error: Option<String>,
    pub feedback: Option<Feedback>,
    #[specta(type = specta_typescript::Number)]
    pub started_at: i64,
    #[specta(type = Option<specta_typescript::Number>)]
    pub ended_at: Option<i64>,
}

impl TurnDto {
    /// The assistant's text across every round, for titles, copy and export.
    #[must_use]
    pub fn assistant_text(&self) -> String {
        self.messages
            .iter()
            .filter(|m| m.role == crate::message::Role::Assistant)
            .map(|m| m.text().trim().to_owned())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

/// Everything the chat view needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct ChatDetail {
    pub id: ChatId,
    pub surface: Surface,
    pub roots: Vec<String>,
    pub title: String,
    pub pinned: bool,
    pub archived: bool,
    pub project_id: Option<ProjectId>,
    #[specta(type = specta_typescript::Number)]
    pub created_at: i64,
    #[specta(type = specta_typescript::Number)]
    pub last_message_at: i64,
    pub model: ModelRef,
    pub mode: Mode,
    pub guard: bool,
    pub effort: ReasoningEffort,
    /// Whether the provider's own web search tool is offered to the model (02 §3).
    pub web_search: bool,
    pub active_turn: Option<TurnId>,
    /// This chat's own standing instructions (10 §2, layer 6). The editor reads them back from
    /// here; the prompt they are frozen into is `system_snapshot`, which the view never sees.
    pub instructions: String,
    pub turns: Vec<TurnDto>,
    /// An incognito session (15 A21). The view reads it to keep the three paths that write a
    /// memory by hand — `/remember`, **Remember this**, the Memory page's own editor — out of a
    /// window whose whole promise is that it keeps nothing.
    pub incognito: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SearchHitKind {
    Chat,
    Message,
}

/// One row of the palette's search (docs/plan/06 §3): a chat by title or a message by text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct SearchHit {
    pub kind: SearchHitKind,
    pub chat_id: ChatId,
    pub chat_title: String,
    pub message_id: Option<MessageId>,
    pub turn_id: Option<TurnId>,
    /// The matched title, or the matching stretch of the message with `…` around it.
    pub snippet: String,
    #[specta(type = specta_typescript::Number)]
    pub ts: i64,
}
