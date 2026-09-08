//! Runtime tools owned by the agent (docs/plan/03 §9, 04 §2): app behaviour exposed to the
//! model under the `gantry__` namespace. They implement the [`Connector`] trait so the turn
//! loop has one call path, but they are not catalog entries: no install, no auth, no process.
//!
//! `gantry__clock` (M3), the artifact tools (M5) and the connector tools (M10: search, access
//! requests, suggestions). Skills and memory follow with their milestones.

pub mod artifacts;
pub mod catalog;
pub mod clock;

use std::sync::Arc;

use async_trait::async_trait;
use gantry_connectors::{
    Connector, ConnectorDescriptor, ConnectorError, ToolCallRequest, ToolEventSink, ToolOutcome,
};
use gantry_core::ToolDef;
use tokio_util::sync::CancellationToken;

use crate::{artifacts::Artifacts, runtime_tools::catalog::ConnectorAccess};

/// The namespace prefix of every runtime tool.
pub const NAMESPACE: &str = "gantry";

pub struct RuntimeTools {
    descriptor: ConnectorDescriptor,
    artifacts: Option<Arc<Artifacts>>,
    connectors: Option<Arc<ConnectorAccess>>,
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
            connectors: None,
        }
    }

    /// With the artifact tools.
    #[must_use]
    pub fn with_artifacts(artifacts: Arc<Artifacts>) -> Self {
        let mut tools = Self::new();
        tools.artifacts = Some(artifacts);
        tools
    }

    /// With the connector tools (03 §9, 04 §9).
    #[must_use]
    pub fn with_connectors(mut self, connectors: Arc<ConnectorAccess>) -> Self {
        self.connectors = Some(connectors);
        self
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
        if let Some(connectors) = &self.connectors {
            defs.extend(connectors.definitions());
        }
        Ok(defs)
    }

    async fn call(
        &self,
        req: ToolCallRequest,
        sink: Arc<dyn ToolEventSink>,
        cancel: CancellationToken,
    ) -> Result<ToolOutcome, ConnectorError> {
        match req.tool.as_str() {
            clock::NAME => Ok(clock::call(&req.args)),
            t if artifacts::NAMES.contains(&t) => match &self.artifacts {
                Some(service) => Ok(artifacts::call(service, &req, &sink).await),
                None => Err(ConnectorError::UnknownTool(t.to_owned())),
            },
            t if catalog::NAMES.contains(&t) => match &self.connectors {
                Some(service) => Ok(service.call(&req, &sink, &cancel).await),
                None => Err(ConnectorError::UnknownTool(t.to_owned())),
            },
            other => Err(ConnectorError::UnknownTool(other.to_owned())),
        }
    }
}
