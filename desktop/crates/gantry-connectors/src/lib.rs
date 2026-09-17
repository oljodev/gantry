//! The Connector trait and registry (docs/plan/03 §4). M3 ships the trait's M3 subset, the
//! registry and the call types; manifests, the native connectors, MCP and OAuth arrive with
//! M6 to M9. Runtime tools owned by `gantry-agent` implement the same trait so the turn loop
//! has one call path.

#![forbid(unsafe_code)]

pub mod auth;
pub mod catalog;
pub mod logs;
pub mod manifest;
pub mod mcp;
pub mod runtime;

use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};

use async_trait::async_trait;
use gantry_core::{
    AgentEventKind, CallId, ChatId, ElicitationAction, ElicitationRequest, InstanceId, Mode,
    ResultPart, ToolDef, TurnId,
};
use tokio_util::sync::CancellationToken;

/// The plan document that specifies this crate.
pub const PLAN_DOCUMENT: &str = "docs/plan/03-connector-system.md";

/// Who a connector is, for namespacing and the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorDescriptor {
    /// The tool namespace prefix: the catalog id, or `gantry` for runtime tools.
    pub id: String,
    pub name: String,
    /// `None` for runtime tools, which are not installed instances.
    pub instance_id: Option<InstanceId>,
    pub first_party: bool,
}

/// What a connector sees of the chat that calls it (read-only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatScope {
    pub chat_id: ChatId,
    /// The turn the call belongs to, so a tool that has to ask the user (03 §9, 04 §9) can
    /// raise an interaction against it.
    pub turn_id: TurnId,
    pub mode: Mode,
    /// Whether attaching an installed connector to this chat has already been decided (04 §9),
    /// so the tool attaches instead of raising a card. In Auto it has been: by the mode when the
    /// guard is off, by the guard before the call ran when it is on. It covers attaching and
    /// nothing else — installing a connector that is not there runs third-party code, and that
    /// is the user's in every mode.
    pub attach_decided: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCallRequest {
    pub call_id: CallId,
    /// Un-namespaced.
    pub tool: String,
    pub args: serde_json::Value,
    pub scope: ChatScope,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolOutcome {
    Complete {
        content: Vec<ResultPart>,
        structured: Option<serde_json::Value>,
        is_error: bool,
        /// Parts the call contributes to the **answer** rather than to its own result (03 §4).
        ///
        /// A picture a tool made is not something to describe in a result and hope the model
        /// quotes: it belongs in the reply, at the point the model asked for it. The turn loop
        /// appends these to the transcript as an assistant message of their own, right after
        /// the results, so they render where the call happened, are parked in the blob store
        /// like any other media, and are dropped on replay to a chat provider — which has
        /// already been shown whatever the result itself carried.
        ///
        /// Empty for every connector but `media`.
        media: Vec<gantry_core::ContentPart>,
    },
}

/// What a connector answers when the user stopped the turn before or during the call (05 §7).
/// One sentence, shared, because the model reads it and three different wordings for one event
/// is three things to understand rather than one.
pub const CANCELLED: &str = "Cancelled by the user.";

impl ToolOutcome {
    /// The call did not finish because the user stopped the turn.
    #[must_use]
    pub fn cancelled() -> Self {
        Self::error(CANCELLED)
    }

    #[must_use]
    pub fn json(value: serde_json::Value) -> Self {
        Self::Complete {
            content: vec![ResultPart::Json {
                json: value.clone(),
            }],
            structured: Some(value),
            is_error: false,
            media: Vec::new(),
        }
    }

    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::Complete {
            content: vec![ResultPart::Text { text: text.into() }],
            structured: None,
            is_error: false,
            media: Vec::new(),
        }
    }

    /// An error the model can read and recover from.
    #[must_use]
    pub fn error(message: impl Into<String>) -> Self {
        Self::Complete {
            content: vec![ResultPart::Text {
                text: message.into(),
            }],
            structured: None,
            is_error: true,
            media: Vec::new(),
        }
    }

    /// The same, carrying media for the answer.
    #[must_use]
    pub fn with_media(mut self, parts: Vec<gantry_core::ContentPart>) -> Self {
        let Self::Complete { media, .. } = &mut self;
        *media = parts;
        self
    }
}

/// Which stream a chunk came from. Defined in `gantry-core` because the event that carries it
/// to the interface is defined there too, and two enums for one fact drift.
pub use gantry_core::ToolStream as OutputStream;

/// Where a running call reports output and progress (05 §3). Every method has a no-op
/// default so a connector implements only what it produces.
#[async_trait::async_trait]
pub trait ToolEventSink: Send + Sync {
    fn output(&self, call_id: &CallId, stream: OutputStream, chunk: &[u8]) {
        let _ = (call_id, stream, chunk);
    }
    fn progress(&self, call_id: &CallId, fraction: Option<f32>, message: Option<String>) {
        let _ = (call_id, fraction, message);
    }
    /// An event of the turn stream a runtime tool produces itself (`artifact.*`, 13 §10).
    fn event(&self, event: AgentEventKind) {
        let _ = event;
    }

    /// A server stopping mid-call to ask the user something (03 §6, MCP's MRTR).
    ///
    /// It rides on the *call's* sink rather than on the session, which is the whole reason this
    /// is here: an elicitation belongs to one tool call in one turn, and a session is shared by
    /// every call a connector makes. rmcp's own `call_tool` walks the rounds through a
    /// session-scoped handler, which cannot know which call it is answering for; `McpConnector`
    /// drives the rounds itself for the same reason.
    ///
    /// Declining is the default, so a sink that has no user attached — an export, a test, the
    /// probe — answers rather than hangs.
    async fn elicit(&self, request: ElicitationRequest) -> ElicitationAnswer {
        let _ = request;
        ElicitationAnswer::declined()
    }
}

/// What came back from the card.
#[derive(Debug, Clone, PartialEq)]
pub struct ElicitationAnswer {
    pub action: ElicitationAction,
    /// The filled-in form. Empty unless the action was `Accept`.
    pub values: serde_json::Value,
}

impl ElicitationAnswer {
    #[must_use]
    pub fn declined() -> Self {
        Self {
            action: ElicitationAction::Decline,
            values: serde_json::Value::Object(serde_json::Map::new()),
        }
    }
}

/// A sink that drops everything.
#[derive(Debug, Default)]
pub struct NoopToolEvents;

impl ToolEventSink for NoopToolEvents {}

#[derive(Debug, thiserror::Error)]
pub enum ConnectorError {
    #[error("unknown tool {0}")]
    UnknownTool(String),
    #[error("invalid arguments: {0}")]
    InvalidArgs(String),
    #[error("{0}")]
    Failed(String),
}

#[async_trait]
pub trait Connector: Send + Sync {
    fn descriptor(&self) -> &ConnectorDescriptor;
    /// The tools with their tiers and flags.
    async fn tools(&self) -> Result<Vec<ToolDef>, ConnectorError>;
    async fn call(
        &self,
        req: ToolCallRequest,
        sink: Arc<dyn ToolEventSink>,
        cancel: CancellationToken,
    ) -> Result<ToolOutcome, ConnectorError>;
}

/// Every connector that can be called, by namespace id. Which of them a given chat may use is
/// decided by `chat_connectors` when the turn's tool set is assembled (03 §11).
#[derive(Default)]
pub struct ConnectorRegistry {
    by_id: RwLock<BTreeMap<String, Arc<dyn Connector>>>,
}

impl ConnectorRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, connector: Arc<dyn Connector>) {
        let id = connector.descriptor().id.clone();
        self.by_id
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, connector);
    }

    /// Drops a connector: uninstalled, disabled, or no longer authorized.
    pub fn remove(&self, id: &str) {
        self.by_id
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id);
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<Arc<dyn Connector>> {
        self.by_id
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .cloned()
    }

    /// In a stable order, so the model-facing tool array is stable across requests.
    #[must_use]
    pub fn list(&self) -> Vec<Arc<dyn Connector>> {
        self.by_id
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect()
    }
}

impl std::fmt::Debug for ConnectorRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ids: Vec<String> = self
            .by_id
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .cloned()
            .collect();
        f.debug_struct("ConnectorRegistry")
            .field("connectors", &ids)
            .finish()
    }
}
