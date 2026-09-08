//! The native connectors: first-party tools that run in this process rather than over MCP
//! (docs/plan/03 §5).
//!
//! One factory, so that "this manifest says `native`" and "there is Rust code behind it" are
//! decided in the same place. The plan put this table inside `gantry-connectors`; it lives here
//! instead, because a connector crate depends on that crate for the `Connector` trait and the
//! registry cannot depend back on it without a cycle (03 §5, corrected 2026-09-08).

use std::sync::Arc;

use gantry_connector_shell::ShellEnv;
use gantry_connectors::Connector;
use gantry_core::{InstanceId, ToolDef};
use gantry_workspace::Workspace;

/// Builds the connector a native manifest names, or `None` when nothing is registered for it —
/// which is how a manifest that ships before its code does stays harmless.
pub fn build(
    catalog_id: &str,
    namespace: String,
    instance_id: InstanceId,
    workspace: &Arc<Workspace>,
    shell_env: &Arc<ShellEnv>,
) -> Option<Arc<dyn Connector>> {
    match catalog_id {
        gantry_connector_filesystem::ID => Some(Arc::new(
            gantry_connector_filesystem::Filesystem::new(namespace, instance_id, workspace.clone()),
        )),
        gantry_connector_code_editor::ID => {
            Some(Arc::new(gantry_connector_code_editor::CodeEditor::new(
                namespace,
                instance_id,
                workspace.clone(),
            )))
        }
        gantry_connector_shell::ID => Some(Arc::new(gantry_connector_shell::Shell::new(
            namespace,
            instance_id,
            workspace.clone(),
            shell_env.clone(),
        ))),
        _ => None,
    }
}

/// The tools a native connector offers, without building one: what **Install** records so the
/// connector's page lists its tools before it has ever been called.
#[must_use]
pub fn definitions(catalog_id: &str) -> Option<Vec<ToolDef>> {
    match catalog_id {
        gantry_connector_filesystem::ID => Some(gantry_connector_filesystem::definitions()),
        gantry_connector_code_editor::ID => Some(gantry_connector_code_editor::definitions()),
        gantry_connector_shell::ID => Some(gantry_connector_shell::definitions()),
        _ => None,
    }
}
