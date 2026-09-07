use gantry_core::{ErrorDto, Settings, SettingsPatch};
use gantry_secrets::SecretStoreStatus;
use gantry_store::repos;
use tauri::{AppHandle, State};
use tauri_specta::Event;

use crate::{AppState, events::SettingsChanged};

#[tauri::command]
#[specta::specta]
pub fn get_settings(state: State<'_, AppState>) -> Result<Settings, ErrorDto> {
    Ok(state
        .settings
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone())
}

/// Replaces the given sections, persists them and returns the whole document.
#[tauri::command]
#[specta::specta]
pub async fn update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    patch: SettingsPatch,
) -> Result<Settings, ErrorDto> {
    let (settings, changed) = {
        let mut s = state.settings.write().unwrap_or_else(|e| e.into_inner());
        let changed = patch.apply(&mut s);
        (s.clone(), changed)
    };
    if changed.is_empty() {
        return Ok(settings);
    }
    let doc = serde_json::to_value(&settings).map_err(|e| ErrorDto::Internal {
        message: e.to_string(),
    })?;
    let rows: Vec<(String, String)> = changed
        .iter()
        .map(|key| ((*key).to_owned(), doc[*key].to_string()))
        .collect();
    state
        .store
        .write(move |conn| {
            for (key, json) in &rows {
                repos::settings::set(conn, key, json)?;
            }
            Ok(())
        })
        .await
        .map_err(gantry_core::GantryError::from)?;
    let _ = SettingsChanged.emit(&app);
    Ok(settings)
}

#[tauri::command]
#[specta::specta]
pub fn get_secret_store_status(state: State<'_, AppState>) -> Result<SecretStoreStatus, ErrorDto> {
    Ok(state.secrets.status().clone())
}
