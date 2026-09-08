//! The Interaction primitive (docs/plan/04 §10): everything a turn can stop and wait for. M3
//! ships the permission prompt; the other kinds join with their milestones.

use serde::{Deserialize, Serialize};

use crate::{
    connector::{AuthType, RuntimeRequirement},
    grant::GrantScope,
    ids::{CallId, ChatId, InstanceId, InteractionId, TurnId},
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
    /// The standing scopes this call may be granted, beyond "allow once" (04 §7, §8).
    pub scopes: Vec<GrantScope>,
}

/// What an access-request card shows (04 §9): a connector that is installed but that this
/// chat has not attached, and the reason the model wants it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct AccessRequest {
    pub instance_id: InstanceId,
    /// The tool namespace, which is the name the model uses.
    pub connector: String,
    pub connector_name: String,
    /// The tools it named; empty means it asked for the connector as a whole.
    pub tools: Vec<String>,
    /// How many tools the connector offers altogether.
    pub tool_count: u32,
    pub reason: String,
}

/// What a connector-suggestion card shows (03 §9): a catalog entry that is not installed at
/// all. Nothing is installed until the user presses the card's button.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ConnectorSuggestion {
    pub catalog_id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub auth: AuthType,
    /// Runtimes the entry needs before it can be installed (03 §11).
    pub requires: Vec<RuntimeRequirement>,
    pub reason: String,
}

/// The kind-specific body of an interaction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InteractionPayload {
    Permission { request: PermissionRequest },
    AccessRequest { request: AccessRequest },
    ConnectorSuggestion { suggestion: ConnectorSuggestion },
}

impl InteractionPayload {
    #[must_use]
    pub fn kind(&self) -> InteractionKind {
        match self {
            InteractionPayload::Permission { .. } => InteractionKind::Permission,
            InteractionPayload::AccessRequest { .. } => InteractionKind::AccessRequest,
            InteractionPayload::ConnectorSuggestion { .. } => InteractionKind::ConnectorSuggestion,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PermissionDecision {
    AllowOnce,
    /// Allow, and remember the answer for the rest of this chat at the given scope (04 §8).
    AllowChat {
        scope: GrantScope,
    },
    Deny,
}

impl PermissionDecision {
    #[must_use]
    pub fn allows(self) -> bool {
        matches!(
            self,
            PermissionDecision::AllowOnce | PermissionDecision::AllowChat { .. }
        )
    }
}

/// The answer to an access request (04 §9). Attaching widens what this chat can reach; it
/// does not decide any single call, which still follows the mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AccessDecision {
    Attach {
        /// Also grant the tools the model named for the rest of the chat.
        allow_tools: bool,
    },
    Deny,
}

/// How a connector suggestion ended (03 §9). The install itself happens in the UI, through the
/// ordinary install flow, and hands back the instance it made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SuggestionOutcome {
    Installed { instance_id: InstanceId },
    Declined,
}

/// How an interaction ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InteractionResolution {
    Permission {
        decision: PermissionDecision,
        /// Shown to the model with a denial.
        message: Option<String>,
    },
    AccessRequest {
        decision: AccessDecision,
        /// Shown to the model with a refusal.
        message: Option<String>,
    },
    ConnectorSuggestion {
        outcome: SuggestionOutcome,
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
                scopes: GrantScope::for_tier(RiskTier::Read),
            },
        };
        assert_eq!(serde_json::to_value(&p).unwrap()["kind"], "permission");
        let chat_grant = InteractionResolution::Permission {
            decision: PermissionDecision::AllowChat {
                scope: GrantScope::AllReads,
            },
            message: None,
        };
        let json = serde_json::to_value(&chat_grant).unwrap();
        assert_eq!(json["decision"]["kind"], "allow_chat");
        assert_eq!(json["decision"]["scope"], "all_reads");
        let r = InteractionResolution::Permission {
            decision: PermissionDecision::Deny,
            message: Some("no".into()),
        };
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["kind"], "permission");
        assert_eq!(json["decision"]["kind"], "deny");
    }
}
