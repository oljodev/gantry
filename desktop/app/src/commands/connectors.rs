//! Browsing, installing, connecting and attaching connectors (docs/plan/03 §10, §11).
//!
//! Nothing here ever returns a secret. A token goes in and is never readable again; the UI sees
//! an auth state and, at most, that a credential is present.

use gantry_core::{
    ChatId, ConnectorConfig, ConnectorInstanceDto, ErrorDto, GantryError, InstanceId, ToolInfo,
};
use gantry_store::repos;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;
use tauri_specta::Event;

use crate::{
    AppState,
    events::{ConnectorsChanged, DeviceCodeNeeded},
};

/// A server the user described by hand, or pasted from another client's configuration (03 §8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct CustomServer {
    pub name: String,
    pub config: ConnectorConfig,
}

#[tauri::command]
#[specta::specta]
pub fn list_catalog(
    state: State<'_, AppState>,
) -> Result<Vec<gantry_core::CatalogEntryDto>, ErrorDto> {
    Ok(state.connectors.browse()?)
}

#[tauri::command]
#[specta::specta]
pub fn list_connectors(state: State<'_, AppState>) -> Result<Vec<ConnectorInstanceDto>, ErrorDto> {
    Ok(state.connectors.instances()?)
}

#[tauri::command]
#[specta::specta]
pub fn get_connector(
    state: State<'_, AppState>,
    instance_id: InstanceId,
) -> Result<ConnectorInstanceDto, ErrorDto> {
    Ok(state.connectors.instance(instance_id)?)
}

/// Installs a catalog entry. A server that needs nothing is connected straight away, so the
/// tool list is on screen before the dialog closes; anything else waits for a credential.
/// Step 1 of a local server's install (03 §11): what it needs, and what this machine has.
///
/// Its own command rather than a field on the catalog entry, because the answer changes while
/// the dialog is open — that is the whole point of **Check again** — and a value frozen into a
/// list that was fetched before the user installed Node would tell them it is still missing.
#[tauri::command]
#[specta::specta]
pub async fn check_runtimes(
    state: State<'_, AppState>,
    catalog_id: String,
) -> Result<Vec<gantry_connectors::runtime::RuntimeStatus>, ErrorDto> {
    Ok(state.connectors.runtime_check(&catalog_id).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn install_connector(
    app: AppHandle,
    state: State<'_, AppState>,
    catalog_id: String,
) -> Result<ConnectorInstanceDto, ErrorDto> {
    let id = state.connectors.install(&catalog_id).await?;
    let instance = state.connectors.instance(id)?;
    if instance.auth == gantry_core::AuthType::None {
        // A first connection that fails is not an install failure: the instance stays, with the
        // error on its card and a Retry beside it.
        if let Err(err) = state.connectors.connect(id).await {
            log::warn!("first connection to {}: {err}", instance.name);
        }
    }
    let _ = ConnectorsChanged.emit(&app);
    Ok(state.connectors.instance(id)?)
}

#[tauri::command]
#[specta::specta]
pub async fn install_custom_connector(
    app: AppHandle,
    state: State<'_, AppState>,
    server: CustomServer,
) -> Result<ConnectorInstanceDto, ErrorDto> {
    if server.name.trim().is_empty() {
        return Err(GantryError::invalid("the server needs a name").into());
    }
    let id = state
        .connectors
        .install_custom(server.name.trim(), server.config)
        .await?;
    if let Err(err) = state.connectors.connect(id).await {
        log::warn!("first connection to {}: {err}", server.name);
    }
    let _ = ConnectorsChanged.emit(&app);
    Ok(state.connectors.instance(id)?)
}

/// Connects and refreshes the tool list.
#[tauri::command]
#[specta::specta]
pub async fn connect_connector(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: InstanceId,
) -> Result<Vec<ToolInfo>, ErrorDto> {
    let tools = state.connectors.connect(instance_id).await?;
    let _ = ConnectorsChanged.emit(&app);
    Ok(tools)
}

/// Runs the browser sign-in end to end: discovery, the browser hand-off, the code, the token,
/// and a first connection. It can take minutes, because a person is in the middle of it.
#[tauri::command]
#[specta::specta]
pub async fn authorize_connector(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: InstanceId,
    client_id: Option<String>,
) -> Result<ConnectorInstanceDto, ErrorDto> {
    let authorization = state
        .connectors
        .begin_authorization(instance_id, client_id)
        .await?;
    let _ = ConnectorsChanged.emit(&app);
    // A device sign-in asks the user to type a code, so the code has to be on screen before the
    // browser takes the focus away.
    if let Some(user_code) = authorization.user_code.clone() {
        let _ = DeviceCodeNeeded {
            instance_id,
            connector: state.connectors.instance(instance_id)?.name,
            user_code,
            verification_uri: authorization.url.clone(),
        }
        .emit(&app);
    }
    app.opener()
        .open_url(authorization.url.clone(), None::<&str>)
        .map_err(|e| GantryError::internal(format!("opening the browser: {e}")))?;
    state
        .connectors
        .finish_authorization(instance_id, authorization)
        .await?;
    let connected = state.connectors.connect(instance_id).await;
    let _ = ConnectorsChanged.emit(&app);
    connected?;
    Ok(state.connectors.instance(instance_id)?)
}

/// Stores a pasted token or key and connects with it.
#[tauri::command]
#[specta::specta]
pub async fn set_connector_token(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: InstanceId,
    token: String,
) -> Result<ConnectorInstanceDto, ErrorDto> {
    state.connectors.set_token(instance_id, &token).await?;
    drop(token);
    let connected = state.connectors.connect(instance_id).await;
    let _ = ConnectorsChanged.emit(&app);
    connected?;
    Ok(state.connectors.instance(instance_id)?)
}

#[tauri::command]
#[specta::specta]
pub async fn set_connector_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: InstanceId,
    enabled: bool,
) -> Result<(), ErrorDto> {
    state.connectors.set_enabled(instance_id, enabled).await?;
    let _ = ConnectorsChanged.emit(&app);
    Ok(())
}

/// Uninstalls: the credentials go, the registered clients go, the history stays readable.
#[tauri::command]
#[specta::specta]
pub async fn remove_connector(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: InstanceId,
) -> Result<(), ErrorDto> {
    state.connectors.remove(instance_id).await?;
    let _ = ConnectorsChanged.emit(&app);
    Ok(())
}

/// Which connectors a chat may use (03 §11: installing something never changes what an
/// existing conversation can reach).
#[tauri::command]
#[specta::specta]
pub fn list_chat_connectors(
    state: State<'_, AppState>,
    chat_id: ChatId,
) -> Result<Vec<InstanceId>, ErrorDto> {
    Ok(state
        .store
        .read(move |c| repos::connectors::attached(c, chat_id))
        .map_err(GantryError::from)?)
}

#[tauri::command]
#[specta::specta]
pub async fn attach_connector(
    app: AppHandle,
    state: State<'_, AppState>,
    chat_id: ChatId,
    instance_id: InstanceId,
    attached: bool,
) -> Result<Vec<InstanceId>, ErrorDto> {
    state
        .store
        .write(move |c| {
            if attached {
                repos::connectors::attach(c, chat_id, instance_id, "user")
            } else {
                repos::connectors::detach(c, chat_id, instance_id)
            }
        })
        .await
        .map_err(GantryError::from)?;
    let _ = ConnectorsChanged.emit(&app);
    Ok(state
        .store
        .read(move |c| repos::connectors::attached(c, chat_id))
        .map_err(GantryError::from)?)
}
