//! Runtime tools owned by the agent (docs/plan/03 §9, 04 §2): app behaviour exposed to the
//! model under the `gantry__` namespace. They implement the [`Connector`] trait so the turn
//! loop has one call path, but they are not catalog entries: no install, no auth, no process.
//!
//! `gantry__clock` (M3) and the artifact tools (M5). Access requests, connector search and
//! suggestions, skills and memory follow with their milestones.

pub mod artifacts;
pub mod clock;

use std::sync::Arc;

use async_trait::async_trait;
use gantry_connectors::{
    Connector, ConnectorDescriptor, ConnectorError, ToolCallRequest, ToolEventSink, ToolOutcome,
};
use gantry_core::ToolDef;
use tokio_util::sync::CancellationToken;

use crate::artifacts::Artifacts;

/// The namespace prefix of every runtime tool.
pub const NAMESPACE: &str = "gantry";

pub struct RuntimeTools {
    descriptor: ConnectorDescriptor,
    artifacts: Option<Arc<Artifacts>>,
}

impl Default for RuntimeTools {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeTools {
    /// The clock only; tests and tool-less contexts.
    #[must_use]
    pub fn new() -> Self {
        Self {
            descriptor: ConnectorDescriptor {
                id: NAMESPACE.to_owned(),
                name: "Gantry".to_owned(),
                instance_id: None,
                first_party: true,
            },
            artifacts: None,
        }
    }

    /// With the artifact tools.
    #[must_use]
    pub fn with_artifacts(artifacts: Arc<Artifacts>) -> Self {
        let mut tools = Self::new();
        tools.artifacts = Some(artifacts);
        tools
    }
}

#[async_trait]
impl Connector for RuntimeTools {
    fn descriptor(&self) -> &ConnectorDescriptor {
        &self.descriptor
    }

    async fn tools(&self) -> Result<Vec<ToolDef>, ConnectorError> {
        let mut defs = vec![clock::definition()];
        if self.artifacts.is_some() {
            defs.extend(artifacts::definitions());
        }
        Ok(defs)
    }

    async fn call(
        &self,
        req: ToolCallRequest,
        sink: Arc<dyn ToolEventSink>,
        _cancel: CancellationToken,
    ) -> Result<ToolOutcome, ConnectorError> {
        match req.tool.as_str() {
            clock::NAME => Ok(clock::call(&req.args)),
            t if artifacts::NAMES.contains(&t) => match &self.artifacts {
                Some(service) => Ok(artifacts::call(service, &req, &sink).await),
                None => Err(ConnectorError::UnknownTool(t.to_owned())),
            },
            other => Err(ConnectorError::UnknownTool(other.to_owned())),
        }
    }
}
