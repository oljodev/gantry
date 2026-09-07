//! The Interaction primitive (docs/plan/04 §10): everything a turn can stop and wait for. M3
//! ships the permission prompt; the other kinds join with their milestones.

use serde::{Deserialize, Serialize};

use crate::{
    ids::{CallId, ChatId, InteractionId, TurnId},
    tool::{RiskTier, ToolDisplay},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum InteractionKind {
    Permission,
    AccessRequest,
    ConnectorSuggestion,
    Elicitation,
    AuthRequired,
    SkillProposal,
    MemoryProposal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum InteractionStatus {
    Pending,
    Resolved,
    Cancelled,
    Expired,
}

/// What a permission card shows (04 §7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct PermissionRequest {
    pub call_id: CallId,
    pub connector: String,
    pub connector_name: String,
    pub tool: String,
    pub model_tool_name: String,
    pub tier: RiskTier,
    #[specta(type = specta_typescript::Unknown)]
    pub args: serde_json::Value,
    pub display: ToolDisplay,
    /// The assistant's last sentence before the call, as "why".
    pub why: Option<String>,
    /// The tool's description, shown on hover.
    pub description: String,
}

/// The kind-specific body of an interaction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InteractionPayload {
    Permission { request: PermissionRequest },
}

impl InteractionPayload {
    #[must_use]
    pub fn kind(&self) -> InteractionKind {
        match self {
            InteractionPayload::Permission { .. } => InteractionKind::Permission,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    AllowOnce,
    Deny,
}

/// How an interaction ended. Grants ("allow for this chat") arrive with M7 as another
/// permission decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InteractionResolution {
    Permission {
        decision: PermissionDecision,
        /// Shown to the model with a denial.
        message: Option<String>,
    },
    /// The turn was cancelled or the app restarted while the card waited.
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Interaction {
    pub id: InteractionId,
    pub chat_id: ChatId,
    pub turn_id: TurnId,
    pub kind: InteractionKind,
    pub payload: InteractionPayload,
    pub status: InteractionStatus,
    pub resolution: Option<InteractionResolution>,
    #[specta(type = specta_typescript::Number)]
    pub created_at: i64,
    #[specta(type = Option<specta_typescript::Number>)]
    pub resolved_at: Option<i64>,
}

impl Interaction {
    #[must_use]
    pub fn pending(chat_id: ChatId, turn_id: TurnId, payload: InteractionPayload) -> Self {
        Self {
            id: InteractionId::new(),
            chat_id,
            turn_id,
            kind: payload.kind(),
            payload,
            status: InteractionStatus::Pending,
            resolution: None,
            created_at: crate::time::now_ms(),
            resolved_at: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_and_resolution_are_tagged_by_kind() {
        let p = InteractionPayload::Permission {
            request: PermissionRequest {
                call_id: CallId::new(),
                connector: "gantry".into(),
                connector_name: "Gantry".into(),
                tool: "clock".into(),
                model_tool_name: "gantry__clock".into(),
                tier: RiskTier::Read,
                args: serde_json::json!({}),
                display: ToolDisplay {
                    kind: crate::tool::ToolDisplayKind::Connector,
                    summary: String::new(),
                },
                why: None,
                description: String::new(),
            },
        };
        assert_eq!(serde_json::to_value(&p).unwrap()["kind"], "permission");
        let r = InteractionResolution::Permission {
            decision: PermissionDecision::Deny,
            message: Some("no".into()),
        };
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["kind"], "permission");
        assert_eq!(json["decision"], "deny");
    }
}
