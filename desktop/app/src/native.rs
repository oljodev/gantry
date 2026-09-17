//! The native connectors: first-party tools that run in this process rather than over MCP
//! (docs/plan/03 §5).
//!
//! One factory, so that "this manifest says `native`" and "there is Rust code behind it" are
//! decided in the same place. The plan put this table inside `gantry-connectors`; it lives here
//! instead, because a connector crate depends on that crate for the `Connector` trait and the
//! registry cannot depend back on it without a cycle (03 §5, corrected 2026-09-08).

use std::sync::{Arc, RwLock};

use gantry_connector_shell::ShellEnv;
use gantry_connectors::Connector;
use gantry_core::{InstanceId, Settings, ToolDef};
use gantry_providers::ProviderRegistry;
use gantry_store::Store;
use gantry_workspace::Workspace;

/// Everything a native connector might be built from, in one place.
///
/// It was a widening argument list until `media` arrived wanting three things nothing else does
/// — the provider clients, their model catalog and the settings cache — and a function whose
/// parameters are six unrelated nouns is one nobody can call without reading it. The connectors
/// that need none of this ignore the rest.
pub struct Deps {
    /// Roots, file IO and the edit journal, shared by every local connector (03 §5).
    pub workspace: Arc<Workspace>,
    /// The login shell and its environment, captured once (`docs/connectors/shell.md` D2).
    pub shell_env: Arc<ShellEnv>,
    /// Where a native connector may keep state that has to survive a restart.
    pub data_dir: std::path::PathBuf,
    /// `media` only: the provider clients, for the endpoints that make a picture (02 §4b).
    pub providers: Arc<ProviderRegistry>,
    /// `media` only: the model catalog lives in the same database.
    pub store: Arc<Store>,
    /// `media` only: what the user chose for a model in the dialog (11 §1).
    pub settings: Arc<RwLock<Settings>>,
}

/// Builds the connector a native manifest names, or `None` when nothing is registered for it —
/// which is how a manifest that ships before its code does stays harmless.
///
/// No native connector asks the user for anything at the moment, so nothing here reads a
/// `user_config` answer. The machinery that fetched them — public values from the instance row,
/// `sensitive` ones from the vault by the field name each was filed under — was written for the
/// `web` connector's search key and removed with it; `git log` has it if a native connector ever
/// needs a setting again.
pub fn build(
    catalog_id: &str,
    namespace: String,
    instance_id: InstanceId,
    deps: &Deps,
) -> Option<Arc<dyn Connector>> {
    let Deps {
        workspace,
        data_dir,
        providers,
        store,
        settings,
        shell_env,
    } = deps;
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
        gantry_connector_media::ID => Some(Arc::new(gantry_connector_media::Media::new(
            namespace,
            instance_id,
            providers.clone(),
            store.clone(),
            settings.clone(),
        ))),
        gantry_connector_web::ID => Some(Arc::new(gantry_connector_web::Web::new(
            namespace,
            instance_id,
            // The search ration lives between runs (`docs/connectors/web.md` §6.4): a restart
            // inside a twenty-minute block used to clear it, and walking back into a block is
            // what extends it.
            Some(data_dir.join("web")),
        ))),
        _ => None,
    }
}

/// The settings form a native connector asks for, with whatever it fills in live (03 §11 step 2).
///
/// Separate from the manifest's `user_config` because the two answer different questions. A
/// manifest can say *which* keys a connector stores; it cannot say what the menu behind one of
/// them contains, when that is the models this user's own keys reach today. So the manifest
/// declares the keys and this fills in the options, and a connector with nothing live to add
/// says `None` and the manifest's own form stands.
#[must_use]
pub fn settings_fields(catalog_id: &str, deps: &Deps) -> Option<Vec<gantry_core::UserConfigField>> {
    match catalog_id {
        gantry_connector_media::ID => Some(gantry_connector_media::settings_fields(
            &deps.providers,
            &deps.store,
        )),
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
        gantry_connector_media::ID => Some(gantry_connector_media::definitions()),
        gantry_connector_web::ID => Some(gantry_connector_web::definitions()),
        _ => None,
    }
}
