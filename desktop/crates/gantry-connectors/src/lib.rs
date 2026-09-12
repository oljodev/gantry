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
use gantry_core::{AgentEventKind, CallId, ChatId, InstanceId, Mode, ResultPart, ToolDef, TurnId};
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
    },
}

impl ToolOutcome {
    #[must_use]
    pub fn json(value: serde_json::Value) -> Self {
        Self::Complete {
            content: vec![ResultPart::Json {
                json: value.clone(),
            }],
            structured: Some(value),
            is_error: false,
        }
    }

    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::Complete {
            content: vec![ResultPart::Text { text: text.into() }],
            structured: None,
            is_error: false,
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
        }
    }
}

/// Which stream a chunk came from. Defined in `gantry-core` because the event that carries it
/// to the interface is defined there too, and two enums for one fact drift.
pub use gantry_core::ToolStream as OutputStream;

/// Where a running call reports output and progress (05 §3). Every method has a no-op
/// default so a connector implements only what it produces.
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
