//! Agent events: what a turn tells its subscribers (docs/plan/05 §2). Every event carries a
//! per-turn sequence number and a timestamp; batches cross IPC on a channel (05 §5).
//!
//! 64-bit fields are exported to TypeScript as `number`; every value stays far below 2^53.
//!
//! M1 carried the text-only subset; M3 adds the tool-call and decision kinds. Artifact and
//! context kinds join with their milestones; adding a variant is additive for every consumer.

use serde::{Deserialize, Serialize};

/// How many lines of a running call's output the app keeps (05 §3): enough that a reattached
/// view sees what a terminal would show, bounded so a command that prints a million lines does
/// not become a million lines of state. The frontend's run store keeps the same window, and the
/// snapshot hands over exactly that.
pub const LIVE_OUTPUT_LINES: usize = 400;

use crate::{
    artifact::VersionSource,
    chat::TurnStatus,
    ids::{ArtifactId, CallId, ChatId, InteractionId, MessageId, TurnId},
    interaction::{Interaction, InteractionResolution},
    message::{ContentPart, Message, ResultPart, Role, StopReason, Usage},
    settings::{Mode, ModelRef},
    tool::{DecisionSource, RiskTier, ToolCallDto, ToolDisplay, ToolStream},
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
    /// One per model round: the first assistant message and every one after a tool round.
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
    /// The model started a tool call; arguments may follow as deltas.
    #[serde(rename = "tool_call.started")]
    ToolCallStarted {
        call_id: CallId,
        message_id: MessageId,
        /// Namespace prefix: a connector id, or `gantry` for runtime tools.
        connector: String,
        connector_name: String,
        tool: String,
        model_tool_name: String,
    },
    #[serde(rename = "tool_call.args_delta")]
    ToolCallArgsDelta { call_id: CallId, fragment: String },
    /// Arguments are complete and the call is classified.
    #[serde(rename = "tool_call.ready")]
    ToolCallReady {
        call_id: CallId,
        #[specta(type = specta_typescript::Unknown)]
        args: serde_json::Value,
        tier: RiskTier,
        display: ToolDisplay,
    },
    /// The turn waits for the user (04 §10).
    #[serde(rename = "decision.requested")]
    // Boxed: an interaction carries a whole permission request and would otherwise set the
    // size of every event in the stream.
    DecisionRequested { interaction: Box<Interaction> },
    #[serde(rename = "decision.resolved")]
    DecisionResolved {
        interaction_id: InteractionId,
        resolution: InteractionResolution,
        source: DecisionSource,
    },
    /// The guard decided about a call (04 §6, §11). Emitted whether it allowed, blocked, or
    /// could not decide and handed the question to the user, because the audit is of the
    /// deciding and not only of the blocking.
    #[serde(rename = "judge.decision")]
    JudgeDecision {
        call_id: CallId,
        verdict: Box<crate::judge::JudgeVerdict>,
    },
    /// The call was allowed and is running; `source` says who allowed it.
    #[serde(rename = "tool_call.executing")]
    ToolCallExecuting {
        call_id: CallId,
        source: DecisionSource,
    },
    /// A running call produced output. Transient: the end state is the call's result and, for
    /// a command, its stored log. This is what makes a long build visibly alive rather than a
    /// spinner (05 §3, `docs/connectors/shell.md` §10).
    #[serde(rename = "tool_call.output")]
    ToolCallOutput {
        call_id: CallId,
        stream: ToolStream,
        chunk: String,
    },
    /// The call ended: with a result, an error result, a denial or a cancellation. The result
    /// content is what the model receives (capped at the transcript limit).
    #[serde(rename = "tool_call.completed")]
    ToolCallCompleted {
        call_id: CallId,
        status: crate::tool::ToolCallStatus,
        /// Who decided, for a call that never ran: a guardrail, the guard, the user, Plan mode.
        /// `None` on a call that did run, because `tool_call.executing` already said so.
        #[serde(default)]
        decision_source: Option<DecisionSource>,
        is_error: bool,
        #[specta(type = specta_typescript::Number)]
        duration_ms: u64,
        result_preview: String,
        result: Vec<ResultPart>,
    },
    #[serde(rename = "provider.notice")]
    ProviderNotice { kind: String, detail: String },
    /// What this turn added to the model's context beyond the transcript (05 §2, 12 §A4, §B4).
    /// The "Context used" row reads it; nothing about the answer depends on it, which is the
    /// point — the user can see what the model was told before it said anything.
    #[serde(rename = "context.injected")]
    ContextInjected {
        injected: crate::memory::InjectedContext,
    },
    /// A runtime tool created an artifact (13 §10).
    #[serde(rename = "artifact.created")]
    ArtifactCreated {
        artifact_id: ArtifactId,
        version: u32,
        artifact_type: String,
        title: String,
    },
    /// A new version of an artifact exists (13 §10).
    #[serde(rename = "artifact.updated")]
    ArtifactUpdated {
        artifact_id: ArtifactId,
        version: u32,
        source: VersionSource,
        title: String,
    },
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
        tool_calls: u32,
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

impl AgentEventKind {
    /// The dotted tag, e.g. `text.delta`.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            AgentEventKind::TurnStarted { .. } => "turn.started",
            AgentEventKind::MessageStarted { .. } => "message.started",
            AgentEventKind::TextDelta { .. } => "text.delta",
            AgentEventKind::ThinkingDelta { .. } => "thinking.delta",
            AgentEventKind::BlockDone { .. } => "block.done",
            AgentEventKind::ToolCallStarted { .. } => "tool_call.started",
            AgentEventKind::ToolCallArgsDelta { .. } => "tool_call.args_delta",
            AgentEventKind::ToolCallReady { .. } => "tool_call.ready",
            AgentEventKind::DecisionRequested { .. } => "decision.requested",
            AgentEventKind::DecisionResolved { .. } => "decision.resolved",
            AgentEventKind::JudgeDecision { .. } => "judge.decision",
            AgentEventKind::ToolCallExecuting { .. } => "tool_call.executing",
            AgentEventKind::ToolCallOutput { .. } => "tool_call.output",
            AgentEventKind::ToolCallCompleted { .. } => "tool_call.completed",
            AgentEventKind::ProviderNotice { .. } => "provider.notice",
            AgentEventKind::ContextInjected { .. } => "context.injected",
            AgentEventKind::ArtifactCreated { .. } => "artifact.created",
            AgentEventKind::ArtifactUpdated { .. } => "artifact.updated",
            AgentEventKind::MessageCompleted { .. } => "message.completed",
            AgentEventKind::TurnCompleted { .. } => "turn.completed",
            AgentEventKind::Error { .. } => "error",
            AgentEventKind::TurnSnapshot { .. } => "turn.snapshot",
        }
    }

    /// Whether the kind belongs to the append-only activity log (05 §2, "persisted").
    #[must_use]
    pub fn is_persisted(&self) -> bool {
        !matches!(
            self,
            AgentEventKind::TextDelta { .. }
                | AgentEventKind::ThinkingDelta { .. }
                | AgentEventKind::BlockDone { .. }
                | AgentEventKind::ToolCallArgsDelta { .. }
                | AgentEventKind::ToolCallOutput { .. }
                | AgentEventKind::TurnSnapshot { .. }
        )
    }
}

/// What a late subscriber needs to draw an in-flight turn: every message of the turn so far
/// (the last one may still be streaming), the tool calls and the pending decisions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct TurnSnapshot {
    pub chat_id: ChatId,
    pub status: TurnStatus,
    /// Assistant and tool messages of the turn in order; parts in block order.
    pub messages: Vec<Message>,
    pub tool_calls: Vec<ToolCallDto>,
    pub pending: Vec<Interaction>,
    /// The last [`LIVE_OUTPUT_LINES`] lines of each call still running, by call id (05 §3).
    ///
    /// A `tool_call.output` event is transient — the end state is the call's result, so nothing
    /// replays it — which left a view that reattached in the middle of a two-minute build
    /// staring at a row with no output until the command finished. The snapshot is the one
    /// place that can answer for the events a subscriber was not there for, so it carries the
    /// same window the view keeps. A finished call is not here: its result has the output.
    pub output: std::collections::HashMap<CallId, Vec<String>>,
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
        assert_eq!(json["type"], e.name());
        assert!(!e.is_persisted());
        let ready = AgentEventKind::ToolCallReady {
            call_id: CallId::new(),
            args: serde_json::json!({}),
            tier: RiskTier::Read,
            display: ToolDisplay {
                kind: crate::tool::ToolDisplayKind::Connector,
                summary: String::new(),
            },
        };
        assert_eq!(serde_json::to_value(&ready).unwrap()["type"], ready.name());
        assert!(ready.is_persisted());
    }
}
