//! Tools as the agent sees them: definitions with a risk tier and flags (docs/plan/03 §3,
//! 04 §2), and the tool-call record the activity feed and the `tool_calls` projection share
//! (05 §1, 06 §3). Model-facing specs (`ToolSpec`) live in `gantry-providers`.

use serde::{Deserialize, Serialize};

use crate::{
    ids::{CallId, ChatId, MessageId, TurnId},
    message::ResultPart,
};

/// What a tool can do to the world (docs/plan/04 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RiskTier {
    /// Observes; no side effects.
    Read,
    /// Mutates local state inside the chat's roots in a way Gantry can revert.
    Write,
    /// Mutates state outside the machine or the roots; not revertible by Gantry.
    WriteExternal,
    /// Runs code with unknown blast radius.
    Execute,
    /// Irreversible deletion or force operations.
    Destructive,
    /// Acts only on Gantry's own state; never prompts, always logged.
    App,
}

impl RiskTier {
    /// How much a tier is trusted with, lowest first, for the `tier_ceiling` of a grant
    /// (04 §8). `App` is outside the ladder: it never asks, so it is never granted.
    #[must_use]
    pub fn rank(self) -> u8 {
        match self {
            RiskTier::App => 0,
            RiskTier::Read => 1,
            RiskTier::Write => 2,
            RiskTier::WriteExternal => 3,
            RiskTier::Execute => 4,
            RiskTier::Destructive => 5,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            RiskTier::Read => "read",
            RiskTier::Write => "write",
            RiskTier::WriteExternal => "write_external",
            RiskTier::Execute => "execute",
            RiskTier::Destructive => "destructive",
            RiskTier::App => "app",
        }
    }

    /// The manifest's default when a tool says nothing (03 §3): the conservative end.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            RiskTier::Read => "read",
            RiskTier::Write => "write",
            RiskTier::WriteExternal => "external write",
            RiskTier::Execute => "execute",
            RiskTier::Destructive => "destructive",
            RiskTier::App => "app",
        }
    }
}

/// What Plan mode does with a tool (docs/plan/04 §4): offer it, hide it, or let a classifier
/// decide per call (the shell).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum PlanModePolicy {
    Allow,
    Deny,
    Classify,
}

/// One callable capability of a connector, with the flags the permission engine reads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct ToolDef {
    /// Un-namespaced, e.g. `read_file`.
    pub name: String,
    pub description: String,
    #[specta(type = specta_typescript::Unknown)]
    pub input_schema: serde_json::Value,
    pub tier: RiskTier,
    /// Prompts even in Unguarded Auto (04 §5).
    pub always_confirm: bool,
    /// May run alongside other calls of the same batch.
    pub parallel_safe: bool,
    pub plan_mode: PlanModePolicy,
    /// Arguments are worth showing while they stream (file contents, patches).
    pub stream_args: bool,
    /// The call widens what this chat can reach rather than acting inside it (04 §9). Attaching
    /// an installed connector is Gantry's own state, so `App` is the right tier and no mode
    /// prompts for the call itself — but the reach it grants is the model's own to ask for, and
    /// in Auto nobody else is looking, so it is the one `App` decision the guard is asked about.
    #[serde(default)]
    pub widens_access: bool,
}

impl ToolDef {
    /// A tool with the conservative flags: not parallel-safe, hidden in Plan mode unless it
    /// reads, no confirmation beyond its tier.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: serde_json::Value,
        tier: RiskTier,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema,
            tier,
            always_confirm: false,
            parallel_safe: false,
            plan_mode: if matches!(tier, RiskTier::Read | RiskTier::App) {
                PlanModePolicy::Allow
            } else {
                PlanModePolicy::Deny
            },
            stream_args: false,
            widens_access: false,
        }
    }
}

/// Which stream a chunk of a running call's output came from. `Log` is the connector's own
/// commentary, which is neither the command's stdout nor its stderr.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ToolStream {
    Stdout,
    Stderr,
    Log,
}

/// Where a tool call is in its life (06 §3, `tool_calls.status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallStatus {
    /// The model asked for it; arguments may still be streaming.
    Proposed,
    AwaitingDecision,
    Denied,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl ToolCallStatus {
    #[must_use]
    pub fn is_final(self) -> bool {
        matches!(
            self,
            ToolCallStatus::Denied
                | ToolCallStatus::Completed
                | ToolCallStatus::Failed
                | ToolCallStatus::Cancelled
        )
    }
}

/// Who or what allowed or refused a call (04 §11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DecisionSource {
    Mode,
    Grant,
    UserOnce,
    UserChatGrant,
    Judge,
    Guardrail,
    Scope,
    PlanMode,
}

/// How the activity row renders a call (05 §2, `tool_call.ready.display`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ToolDisplayKind {
    Edit,
    Command,
    Connector,
    Read,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ToolDisplay {
    pub kind: ToolDisplayKind,
    /// A one-line argument summary in the row, e.g. `path=src/app.rs` or `$ npm test`.
    pub summary: String,
}

/// A tool call as the activity feed and the detail pane see it: the `tool_calls` row plus the
/// result kept in the transcript.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct ToolCallDto {
    pub id: CallId,
    pub chat_id: ChatId,
    pub turn_id: TurnId,
    /// The assistant message that requested the call.
    pub message_id: MessageId,
    /// The namespace prefix the model used: a connector id, or `gantry` for runtime tools.
    pub connector: String,
    pub connector_name: String,
    /// Un-namespaced tool name.
    pub tool: String,
    /// The name the model used, e.g. `gantry__clock`.
    pub model_tool_name: String,
    #[specta(type = specta_typescript::Unknown)]
    pub args: serde_json::Value,
    pub tier: RiskTier,
    pub status: ToolCallStatus,
    pub decision_source: Option<DecisionSource>,
    /// What the guard decided, when the guard was asked (04 §6). Present on an allow as well
    /// as on a block: the row shows a "guard ✓" mark with the reason on hover.
    pub judge: Option<crate::judge::JudgeVerdict>,
    pub display: ToolDisplay,
    /// The first part of the result as text, for the row and the projection.
    pub result_preview: Option<String>,
    /// The full result when it is known: from the transcript for a finished turn, from the
    /// completion event while the turn runs. Cut to `advanced.max_result_kb`, like the
    /// transcript's own copy — `result_blob_hash` is the uncut one.
    pub result: Option<Vec<ResultPart>>,
    /// The whole output as a blob, set when the result above had to be cut (05 §8, 06 §3).
    /// `tool_call_output` reads it; the sweep keeps it for as long as this row lives.
    pub result_blob_hash: Option<String>,
    pub is_error: bool,
    #[specta(type = Option<specta_typescript::Number>)]
    pub started_at: Option<i64>,
    #[specta(type = Option<specta_typescript::Number>)]
    pub ended_at: Option<i64>,
    #[specta(type = Option<specta_typescript::Number>)]
    pub duration_ms: Option<u64>,
}

/// The text of a result, capped, for `result_preview` and the row.
#[must_use]
pub fn result_preview(content: &[ResultPart], max_chars: usize) -> String {
    let text: String = content
        .iter()
        .map(|p| match p {
            ResultPart::Text { text } => text.clone(),
            ResultPart::Json { json } => json.to_string(),
            ResultPart::Image { mime, .. } => format!("[image {mime}]"),
            ResultPart::Resource { summary, .. } => summary.clone(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    if text.chars().count() <= max_chars {
        text
    } else {
        let mut cut: String = text.chars().take(max_chars).collect();
        cut.push('…');
        cut
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_serialise_as_snake_case() {
        assert_eq!(
            serde_json::to_value(RiskTier::WriteExternal).unwrap(),
            "write_external"
        );
        let t = ToolDef::new("x", "d", serde_json::json!({}), RiskTier::Write);
        assert_eq!(t.plan_mode, PlanModePolicy::Deny);
        let r = ToolDef::new("y", "d", serde_json::json!({}), RiskTier::Read);
        assert_eq!(r.plan_mode, PlanModePolicy::Allow);
    }

    #[test]
    fn previews_are_capped_with_an_ellipsis() {
        let parts = vec![ResultPart::Text {
            text: "abcdefgh".into(),
        }];
        assert_eq!(result_preview(&parts, 3), "abc…");
        assert_eq!(result_preview(&parts, 8), "abcdefgh");
    }
}
