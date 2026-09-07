//! Runtime tools owned by the agent (docs/plan/03 §9, 04 §2): app behaviour exposed to the
//! model under the `gantry__` namespace. They implement the [`Connector`] trait so the turn
//! loop has one call path, but they are not catalog entries: no install, no auth, no process.
//!
//! M3 ships `gantry__clock`. Access requests, connector search and suggestions, artifacts,
//! skills and memory follow with their milestones.

pub mod clock;

use std::sync::Arc;

use async_trait::async_trait;
use gantry_connectors::{
    Connector, ConnectorDescriptor, ConnectorError, ToolCallRequest, ToolEventSink, ToolOutcome,
};
use gantry_core::ToolDef;
use tokio_util::sync::CancellationToken;

/// The namespace prefix of every runtime tool.
pub const NAMESPACE: &str = "gantry";

pub struct RuntimeTools {
    descriptor: ConnectorDescriptor,
}

impl Default for RuntimeTools {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeTools {
    #[must_use]
    pub fn new() -> Self {
        Self {
            descriptor: ConnectorDescriptor {
                id: NAMESPACE.to_owned(),
                name: "Gantry".to_owned(),
                instance_id: None,
                first_party: true,
            },
        }
    }
}

#[async_trait]
impl Connector for RuntimeTools {
    fn descriptor(&self) -> &ConnectorDescriptor {
        &self.descriptor
    }

    async fn tools(&self) -> Result<Vec<ToolDef>, ConnectorError> {
        Ok(vec![clock::definition()])
    }

    async fn call(
        &self,
        req: ToolCallRequest,
        _sink: Arc<dyn ToolEventSink>,
        _cancel: CancellationToken,
    ) -> Result<ToolOutcome, ConnectorError> {
        match req.tool.as_str() {
            clock::NAME => Ok(clock::call(&req.args)),
            other => Err(ConnectorError::UnknownTool(other.to_owned())),
        }
    }
}
