//! The Interaction primitive (docs/plan/04 §10): everything a turn can stop and wait for. M3
//! ships the permission prompt; the other kinds join with their milestones.

use serde::{Deserialize, Serialize};

use crate::{
    connector::{AuthType, RuntimeRequirement},
    grant::GrantScope,
    ids::{CallId, ChatId, InstanceId, InteractionId, TurnId},
    memory::{MemoryProposal, MemoryProposalOutcome},
    skill::{SkillProposal, SkillProposalOutcome},
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
    /// The guardrail that raised this prompt, when one did (04 §5). The card leads with its
    /// reason, because "Gantry always asks about this, and here is why" is the whole message.
    pub guardrail: Option<crate::guardrail::GuardrailHit>,
    /// Why the guard did not answer this one itself (04 §6): it timed out, it could not be
    /// reached, or it allowed a `destructive` call without being sure enough. The card leads
    /// with this for the same reason it leads with a guardrail: in Auto mode, being asked at
    /// all is the surprising part, so the first thing to say is why.
    pub guard: Option<String>,
    /// The standing scopes this call may be granted, beyond "allow once" (04 §7, §8).
    pub scopes: Vec<GrantScope>,
    /// Arguments the card lets the user change before the call runs (04 §7).
    ///
    /// Asked of the connector when the card is raised, so it is the connector — the only thing
    /// that knows what its own arguments mean — that decides what is worth offering. Empty for
    /// every tool that offers nothing, which is nearly all of them.
    #[serde(default)]
    pub choices: Vec<ArgChoice>,
}

/// One argument a permission card offers to change, with what to change it to.
///
/// The case it was built for is `media__generate`'s model. The card already names the model that
/// is about to spend money; showing the name and not letting the user change it is the one
/// unhelpful arrangement — deny and re-ask is the only way through, and it costs a whole round
/// trip through the chat model to say "use that one instead".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ArgChoice {
    /// The argument's name, as the tool's own schema spells it.
    pub key: String,
    /// What to call it on the card: "Model", not `model`.
    pub label: String,
    /// The value the call will use unless the user changes it. Already resolved: what the
    /// connector *would* do, not what the model literally typed, so the card shows the thing
    /// that is about to happen.
    pub value: Option<String>,
    /// What it may be changed to. A choice with none is shown but not editable.
    pub options: Vec<ChoiceOption>,
    /// Why the value is not what the model asked for, when it is not — a model it named that
    /// this machine does not have, a default that stood in. Shown on the card as a warning,
    /// because a silent substitution is the one thing a card like this must not do.
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ChoiceOption {
    pub value: String,
    pub label: String,
    /// The price, the provider, the small print: the reason to pick this one.
    pub detail: Option<String>,
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

/// What a server asked the user for, in the middle of a tool call (03 §6, MCP's MRTR).
///
/// A tool can stop halfway and say it needs something only a person can give — which repository,
/// which of these three accounts, are you sure. The call is not finished and not failed: it is
/// waiting, and the answer goes back as the next round of the same call.
///
/// Modelled in Gantry's own terms rather than the wire's, because this crate knows nothing about
/// MCP and because only the primitives survive the translation anyway: the specification allows a
/// flat object of strings, numbers and booleans, and nothing nested.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct ElicitationRequest {
    /// The call that is waiting, so the card can sit with the row it belongs to.
    pub call_id: CallId,
    pub connector: String,
    pub connector_name: String,
    /// The server's own sentence about what it needs. Untrusted text from a third party — shown
    /// as the server's words, never as Gantry's.
    pub message: String,
    pub fields: Vec<ElicitationField>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct ElicitationField {
    pub key: String,
    pub kind: ElicitationFieldKind,
    pub title: String,
    pub description: Option<String>,
    pub required: bool,
    /// The values a choice is between, with the labels the server gave them.
    pub options: Vec<ElicitationOption>,
    /// `email`, `uri`, `date`, `date-time` — what the server said the string is.
    pub format: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ElicitationFieldKind {
    String,
    Number,
    Integer,
    Boolean,
    Enum,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ElicitationOption {
    pub value: String,
    pub label: String,
}

/// What the user did with an elicitation card (MCP's three actions).
///
/// `Decline` and `Cancel` are different answers and the server is told which: declining is "no,
/// carry on without it", cancelling is "stop, I am not answering this" — a distinction the
/// specification makes and a server may act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ElicitationAction {
    Accept,
    Decline,
    Cancel,
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
    /// Boxed like the two proposals below it: the request is much the largest of the payloads,
    /// and every interaction of every other kind would otherwise carry its size around.
    Permission {
        request: Box<PermissionRequest>,
    },
    AccessRequest {
        request: AccessRequest,
    },
    ConnectorSuggestion {
        suggestion: ConnectorSuggestion,
    },
    Elicitation {
        request: ElicitationRequest,
    },
    /// A skill the model wrote (12 §A5). Unlike every payload above it, the turn does not wait
    /// for this one: the card sits in the feed and the model carries on.
    SkillProposal {
        proposal: Box<SkillProposal>,
    },
    /// Something the model would remember, or forget (12 §B3). Also non-blocking.
    MemoryProposal {
        proposal: Box<MemoryProposal>,
    },
}

impl InteractionPayload {
    #[must_use]
    pub fn kind(&self) -> InteractionKind {
        match self {
            InteractionPayload::Permission { .. } => InteractionKind::Permission,
            InteractionPayload::AccessRequest { .. } => InteractionKind::AccessRequest,
            InteractionPayload::ConnectorSuggestion { .. } => InteractionKind::ConnectorSuggestion,
            InteractionPayload::Elicitation { .. } => InteractionKind::Elicitation,
            InteractionPayload::SkillProposal { .. } => InteractionKind::SkillProposal,
            InteractionPayload::MemoryProposal { .. } => InteractionKind::MemoryProposal,
        }
    }

    /// Whether the turn stops until this is answered. A proposal is an offer, not a question
    /// the work depends on, so the model is told what happened on its next turn instead
    /// (12 §A5, §B3).
    #[must_use]
    pub fn blocks_the_turn(&self) -> bool {
        !matches!(
            self,
            InteractionPayload::SkillProposal { .. } | InteractionPayload::MemoryProposal { .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
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
    pub fn allows(&self) -> bool {
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
        /// What the user chose for the request's `choices`, by argument key (04 §7). Only keys
        /// the request offered are honoured, and only values it listed: the card widens what a
        /// call may be *allowed* to do by nothing, it only picks between things the connector
        /// already said were equivalent.
        #[serde(default)]
        chosen: std::collections::BTreeMap<String, String>,
    },
    AccessRequest {
        decision: AccessDecision,
        /// Shown to the model with a refusal.
        message: Option<String>,
    },
    ConnectorSuggestion {
        outcome: SuggestionOutcome,
    },
    Elicitation {
        action: ElicitationAction,
        /// The filled-in form, by key. Empty unless the action was `Accept`.
        #[specta(type = specta_typescript::Unknown)]
        values: serde_json::Value,
    },
    SkillProposal {
        outcome: SkillProposalOutcome,
    },
    MemoryProposal {
        outcome: MemoryProposalOutcome,
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
            request: Box::new(PermissionRequest {
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
                guardrail: None,
                guard: None,
                scopes: GrantScope::for_call(RiskTier::Read, &serde_json::json!({}), None, false),
                choices: Vec::new(),
            }),
        };
        assert_eq!(serde_json::to_value(&p).unwrap()["kind"], "permission");
        let chat_grant = InteractionResolution::Permission {
            decision: PermissionDecision::AllowChat {
                scope: GrantScope::AllReads,
            },
            message: None,
            chosen: std::collections::BTreeMap::new(),
        };
        let json = serde_json::to_value(&chat_grant).unwrap();
        assert_eq!(json["decision"]["kind"], "allow_chat");
        // The scope is tagged too since it grew a payload: an argument scope carries the prefix
        // it will grant, so the card can show exactly what the user is about to agree to.
        assert_eq!(json["decision"]["scope"]["kind"], "all_reads");
        let scoped = InteractionResolution::Permission {
            decision: PermissionDecision::AllowChat {
                scope: GrantScope::PathPrefix {
                    prefix: "/home/olav/dev/gantry".into(),
                },
            },
            message: None,
            chosen: std::collections::BTreeMap::new(),
        };
        let json = serde_json::to_value(&scoped).unwrap();
        assert_eq!(json["decision"]["scope"]["kind"], "path_prefix");
        assert_eq!(json["decision"]["scope"]["prefix"], "/home/olav/dev/gantry");
        let r = InteractionResolution::Permission {
            decision: PermissionDecision::Deny,
            message: Some("no".into()),
            chosen: std::collections::BTreeMap::new(),
        };
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["kind"], "permission");
        assert_eq!(json["decision"]["kind"], "deny");
    }
}
