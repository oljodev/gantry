//! Runtime tools owned by the agent (docs/plan/03 §9, 04 §2): app behaviour exposed to the
//! model under the `gantry__` namespace. They implement the [`Connector`] trait so the turn
//! loop has one call path, but they are not catalog entries: no install, no auth, no process.
//!
//! `gantry__clock` (M3), the artifact tools (M5), the connector tools (M10: search, access
//! requests, suggestions), with M12 the skill and memory tools (12 §A7, §B7), and
//! `gantry__update_todos`, the checklist a model keeps for a task of several steps (03 §9b).

pub mod artifacts;
pub mod catalog;
pub mod clock;
pub mod memory;
pub mod skills;
pub mod todos;

use std::sync::Arc;

use async_trait::async_trait;
use gantry_connectors::{
    Connector, ConnectorDescriptor, ConnectorError, ToolCallRequest, ToolEventSink, ToolOutcome,
};
use gantry_core::ToolDef;
use tokio_util::sync::CancellationToken;

use crate::{
    artifacts::Artifacts,
    runtime_tools::{catalog::ConnectorAccess, memory::MemoryTools, skills::SkillTools},
};

/// The namespace prefix of every runtime tool.
pub const NAMESPACE: &str = "gantry";

pub struct RuntimeTools {
    descriptor: ConnectorDescriptor,
    artifacts: Option<Arc<Artifacts>>,
    connectors: Option<Arc<ConnectorAccess>>,
    skills: Option<Arc<SkillTools>>,
    memory: Option<Arc<MemoryTools>>,
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
            skills: None,
            memory: None,
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

    /// With the skill and memory tools (12 §A7, §B7).
    #[must_use]
    pub fn with_library(mut self, skills: Arc<SkillTools>, memory: Arc<MemoryTools>) -> Self {
        self.skills = Some(skills);
        self.memory = Some(memory);
        self
    }
}

#[async_trait]
impl Connector for RuntimeTools {
    fn descriptor(&self) -> &ConnectorDescriptor {
        &self.descriptor
    }

    async fn tools(&self) -> Result<Vec<ToolDef>, ConnectorError> {
        let mut defs = vec![clock::definition(), todos::definition()];
        if self.artifacts.is_some() {
            defs.extend(artifacts::definitions());
        }
        if let Some(connectors) = &self.connectors {
            defs.extend(connectors.definitions());
        }
        if let Some(skills) = &self.skills {
            defs.extend(skills.definitions());
        }
        if let Some(memory) = &self.memory {
            defs.extend(memory.definitions());
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
            todos::NAME => Ok(todos::call(&req.args)),
            t if artifacts::NAMES.contains(&t) => match &self.artifacts {
                Some(service) => Ok(artifacts::call(service, &req, &sink).await),
                None => Err(ConnectorError::UnknownTool(t.to_owned())),
            },
            t if catalog::NAMES.contains(&t) => match &self.connectors {
                Some(service) => Ok(service.call(&req, &sink, &cancel).await),
                None => Err(ConnectorError::UnknownTool(t.to_owned())),
            },
            t if skills::NAMES.contains(&t) => match &self.skills {
                Some(service) => Ok(service.call(&req, &sink)),
                None => Err(ConnectorError::UnknownTool(t.to_owned())),
            },
            t if memory::NAMES.contains(&t) => match &self.memory {
                Some(service) => Ok(service.call(&req, &sink)),
                None => Err(ConnectorError::UnknownTool(t.to_owned())),
            },
            other => Err(ConnectorError::UnknownTool(other.to_owned())),
        }
    }
}
