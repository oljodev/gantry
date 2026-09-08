//! First-party connector: Filesystem (`docs/connectors/filesystem.md`).
//!
//! Reading is here; changing a file in place is the code editor's. Both sit on
//! `gantry-workspace`, which owns the roots, the path rules and the record of what this session
//! has seen — so a read here is what makes an edit there possible.
//!
//! Built so far: `read_file`. The rest of §5 — listing, `stat`, `glob`, `grep`, and the writing
//! tools — arrives with the rest of M6; this is the half the code editor cannot work without.

#![forbid(unsafe_code)]

use std::sync::Arc;

use async_trait::async_trait;
use gantry_connectors::{
    Connector, ConnectorDescriptor, ConnectorError, ToolCallRequest, ToolEventSink, ToolOutcome,
};
use gantry_core::{InstanceId, RiskTier, ToolDef};
use gantry_workspace::Workspace;
use tokio_util::sync::CancellationToken;

/// The connector manifest, embedded at build time (`docs/plan/03-connector-system.md` §3).
pub const MANIFEST: &str = include_str!("../manifest.json");

/// The connector id; equals the folder name and the tool namespace prefix.
pub const ID: &str = "filesystem";

/// Lines returned when the model does not say. Enough for most source files, small enough that
/// a stray read of something enormous does not cost a turn's context.
const DEFAULT_LIMIT: usize = 2000;

pub struct Filesystem {
    descriptor: ConnectorDescriptor,
    workspace: Arc<Workspace>,
}

impl Filesystem {
    #[must_use]
    pub fn new(namespace: String, instance_id: InstanceId, workspace: Arc<Workspace>) -> Self {
        Self {
            descriptor: ConnectorDescriptor {
                id: namespace,
                name: "Filesystem".to_owned(),
                instance_id: Some(instance_id),
                first_party: true,
            },
            workspace,
        }
    }
}

#[async_trait]
impl Connector for Filesystem {
    fn descriptor(&self) -> &ConnectorDescriptor {
        &self.descriptor
    }

    async fn tools(&self) -> Result<Vec<ToolDef>, ConnectorError> {
        Ok(definitions())
    }

    async fn call(
        &self,
        req: ToolCallRequest,
        _sink: Arc<dyn ToolEventSink>,
        _cancel: CancellationToken,
    ) -> Result<ToolOutcome, ConnectorError> {
        if req.tool != "read_file" {
            return Err(ConnectorError::UnknownTool(req.tool));
        }
        let path = req
            .args
            .get("path")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ConnectorError::InvalidArgs("`path` is required".to_owned()))?;
        let offset = req
            .args
            .get("offset")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as usize;
        let limit = req
            .args
            .get("limit")
            .and_then(serde_json::Value::as_u64)
            .map_or(DEFAULT_LIMIT, |n| n.max(1) as usize);

        let roots = match self.workspace.roots(req.scope.chat_id) {
            Ok(roots) => roots,
            Err(err) => return Ok(ToolOutcome::error(err.to_string())),
        };
        let read = match self.workspace.read(&roots, req.scope.chat_id, path) {
            Ok(read) => read,
            Err(err) => return Ok(ToolOutcome::error(err.to_string())),
        };

        let lines: Vec<&str> = read.file.text.lines().collect();
        let total = lines.len();
        let end = offset.saturating_add(limit).min(total);
        let shown = lines.get(offset..end).unwrap_or_default().join("\n");

        // Position is metadata, never a prefix inside the text (D8): a line number in the
        // content bleeds into what the model writes back, and shows up as an edit indented one
        // level too deep. Nothing is silently truncated either (D9).
        Ok(ToolOutcome::json(serde_json::json!({
            "path": read.scoped.path.display().to_string(),
            "content": shown,
            "first_line": offset + 1,
            "last_line": end,
            "total_lines": total,
            "more": end < total,
            "encoding": read.file.encoding(),
            "line_ending": read.file.line_ending(),
        })))
    }
}

#[must_use]
pub fn definitions() -> Vec<ToolDef> {
    let mut read = ToolDef::new(
        "read_file",
        "Read the text of a file inside a folder attached to this chat. Read a file before \
         editing it. Line positions come back as fields, not inside the text.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Absolute path of the file to read." },
                "offset": { "type": "integer", "minimum": 0, "default": 0,
                            "description": "First line to return, counting from 0." },
                "limit": { "type": "integer", "minimum": 1, "default": DEFAULT_LIMIT,
                           "description": "How many lines to return." }
            },
            "required": ["path"],
            "additionalProperties": false
        }),
        RiskTier::Read,
    );
    read.parallel_safe = true;
    vec![read]
}

#[cfg(test)]
mod tests {
    #[test]
    fn manifest_is_valid_json_with_the_right_id() {
        let manifest: serde_json::Value = serde_json::from_str(super::MANIFEST).unwrap();
        assert_eq!(manifest["manifest_version"], "1");
        assert_eq!(manifest["id"], super::ID);
        assert_eq!(manifest["runtime"]["kind"], "native");
        assert_eq!(manifest["runtime"]["crate"], env!("CARGO_PKG_NAME"));
    }

    #[test]
    fn reading_is_a_read_tier_tool_that_may_run_beside_others() {
        let defs = super::definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].tier, gantry_core::RiskTier::Read);
        assert!(defs[0].parallel_safe);
    }
}
