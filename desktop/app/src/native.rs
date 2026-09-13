//! The native connectors: first-party tools that run in this process rather than over MCP
//! (docs/plan/03 §5).
//!
//! One factory, so that "this manifest says `native`" and "there is Rust code behind it" are
//! decided in the same place. The plan put this table inside `gantry-connectors`; it lives here
//! instead, because a connector crate depends on that crate for the `Connector` trait and the
//! registry cannot depend back on it without a cycle (03 §5, corrected 2026-09-08).

use std::{collections::BTreeMap, sync::Arc};

use gantry_connector_shell::ShellEnv;
use gantry_connectors::Connector;
use gantry_core::{InstanceId, ToolDef};
use gantry_secrets::SecretString;
use gantry_workspace::Workspace;

/// The answers to a native connector's `user_config` form (03 §11 step 2).
///
/// Two maps rather than one, because the two halves are kept in different places and the
/// difference is the point: a `sensitive` answer never touches the instance row, so it arrives
/// from the vault or not at all (06 §3). A connector that reads `secrets` is reading something
/// the database cannot show anybody.
#[derive(Debug, Default)]
pub struct NativeConfig {
    /// What the instance row holds, by field name.
    pub public: BTreeMap<String, String>,
    /// What the vault holds, by the field name it was filed under.
    pub secrets: BTreeMap<String, SecretString>,
}

impl NativeConfig {
    fn text(&self, key: &str) -> Option<&str> {
        self.public.get(key).map(String::as_str)
    }
}

/// Builds the connector a native manifest names, or `None` when nothing is registered for it —
/// which is how a manifest that ships before its code does stays harmless.
pub fn build(
    catalog_id: &str,
    namespace: String,
    instance_id: InstanceId,
    workspace: &Arc<Workspace>,
    shell_env: &Arc<ShellEnv>,
    config: &NativeConfig,
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
        gantry_connector_web::ID => Some(Arc::new(gantry_connector_web::Web::new(
            namespace,
            instance_id,
            web_search(config),
        ))),
        _ => None,
    }
}

/// The tools a native connector offers, without building one: what **Install** records so the
/// connector's page lists its tools before it has ever been called.
///
/// It takes the configuration for the same reason `build` does — `web` offers `search` only once
/// there is a key, so a list written without the answers would promise a tool that is not there,
/// or hide one that is.
#[must_use]
pub fn definitions(catalog_id: &str, config: &NativeConfig) -> Option<Vec<ToolDef>> {
    match catalog_id {
        gantry_connector_filesystem::ID => Some(gantry_connector_filesystem::definitions()),
        gantry_connector_code_editor::ID => Some(gantry_connector_code_editor::definitions()),
        gantry_connector_shell::ID => Some(gantry_connector_shell::definitions()),
        gantry_connector_web::ID => Some(gantry_connector_web::definitions(
            web_search(config).is_some(),
        )),
        _ => None,
    }
}

/// The `web` connector's search key, if the user has configured one (03 §5, §11 step 2).
///
/// Bring your own key: Gantry has no search account, so `search` exists for a given instance
/// only when that instance has been given a key and a provider to spend it at. Both answers are
/// required and neither is guessed — a key pasted against the wrong provider name would be sent
/// to a service that did not issue it.
fn web_search(config: &NativeConfig) -> Option<gantry_connector_web::Search> {
    gantry_connector_web::Search::from_config(
        config.text("SEARCH_PROVIDER"),
        config.secrets.get("SEARCH_API_KEY"),
    )
}
