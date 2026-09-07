//! Agent events: what a turn tells its subscribers (docs/plan/05 §2). Every event carries a
//! per-turn sequence number and a timestamp; batches cross IPC on a channel (05 §5).
//!
//! 64-bit fields are exported to TypeScript as `number`; every value stays far below 2^53.
//!
//! M1 carries the text-only subset. Tool, decision and artifact kinds join with their
//! milestones; adding a variant is additive for every consumer.

use serde::{Deserialize, Serialize};

use crate::{
    chat::TurnStatus,
    ids::{ChatId, MessageId, TurnId},
    message::{ContentPart, Role, StopReason, Usage},
    settings::{Mode, ModelRef},
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct AgentEvent {
    /// Monotonic within the turn, starting at 1.
    pub seq: u32,
    /// Milliseconds since the Unix epoch.
    #[specta(type = specta_typescript::Number)]
    pub ts: i64,
    pub turn_id: TurnId,
    pub event: AgentEventKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type")]
pub enum AgentEventKind {
    #[serde(rename = "turn.started")]
    TurnStarted {
        chat_id: ChatId,
        mode: Mode,
        guard: bool,
        model: ModelRef,
    },
    #[serde(rename = "message.started")]
    MessageStarted { message_id: MessageId, role: Role },
    #[serde(rename = "text.delta")]
    TextDelta {
        message_id: MessageId,
        block: u32,
        text: String,
    },
    #[serde(rename = "thinking.delta")]
    ThinkingDelta {
        message_id: MessageId,
        block: u32,
        text: String,
    },
    /// A block is complete; its final part is authoritative.
    #[serde(rename = "block.done")]
    BlockDone {
        message_id: MessageId,
        block: u32,
        part: ContentPart,
    },
    #[serde(rename = "provider.notice")]
    ProviderNotice { kind: String, detail: String },
    #[serde(rename = "message.completed")]
    MessageCompleted {
        message_id: MessageId,
        stop_reason: StopReason,
        usage: Option<Usage>,
    },
    #[serde(rename = "turn.completed")]
    TurnCompleted {
        status: TurnStatus,
        usage: Option<Usage>,
        #[specta(type = specta_typescript::Number)]
        duration_ms: u64,
    },
    #[serde(rename = "error")]
    Error {
        code: String,
        message: String,
        retryable: bool,
    },
    /// The whole current state of an active turn; first on `subscribe_turn`.
    #[serde(rename = "turn.snapshot")]
    TurnSnapshot { snapshot: TurnSnapshot },
}

/// What a late subscriber needs to draw an in-flight turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct TurnSnapshot {
    pub chat_id: ChatId,
    pub status: TurnStatus,
    pub message_id: Option<MessageId>,
    /// The assistant parts accumulated so far, in block order.
    pub parts: Vec<ContentPart>,
    pub usage: Option<Usage>,
    #[specta(type = specta_typescript::Number)]
    pub started_at: i64,
    /// The last `seq` this snapshot covers; live events continue from `seq + 1`.
    pub seq: u32,
}

/// The wire unit on the channel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct AgentEventBatch {
    pub turn_id: TurnId,
    pub events: Vec<AgentEvent>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_are_tagged_with_dotted_names() {
        let e = AgentEventKind::TextDelta {
            message_id: MessageId::new(),
            block: 0,
            text: "hi".into(),
        };
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["type"], "text.delta");
    }
}
