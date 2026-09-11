use gantry_core::{ErrorDto, GantryError, GuardrailRule, Guardrails, Settings, SettingsPatch};
use gantry_secrets::SecretStoreStatus;
use gantry_store::repos;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;
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

/// Replaces the given sections, persists them and returns the whole document. A change to the
/// global custom instructions reaches every open chat as a `SystemNote` (10 §4).
#[tauri::command]
#[specta::specta]
pub async fn update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    patch: SettingsPatch,
) -> Result<Settings, ErrorDto> {
    let (settings, changed, instructions_changed) = {
        let mut s = state.settings.write().unwrap_or_else(|e| e.into_inner());
        let before = s.chat.custom_instructions.trim().to_owned();
        let changed = patch.apply(&mut s);
        let after = s.chat.custom_instructions.trim().to_owned();
        (s.clone(), changed, before != after)
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
        .map_err(GantryError::from)?;
    if instructions_changed {
        let turns = state.turns.clone();
        tauri::async_runtime::spawn_blocking(move || turns.global_instructions_changed())
            .await
            .map_err(|e| GantryError::internal(e.to_string()))??;
    }
    let _ = SettingsChanged.emit(&app);
    Ok(settings)
}

/// What Settings → Guard & guardrails needs beyond the settings document: the rules the app
/// ships with (04 §5), so the page can list them beside the user's own and show which of them
/// are switched off, and anything in force that will not compile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct GuardrailInfo {
    pub shipped: Vec<GuardrailRule>,
    /// Rules in force that were skipped, as `id: what is wrong with the pattern`.
    pub problems: Vec<String>,
}

#[tauri::command]
#[specta::specta]
pub fn get_guardrails(state: State<'_, AppState>) -> Result<GuardrailInfo, ErrorDto> {
    let settings = state.settings.read().unwrap_or_else(|e| e.into_inner());
    Ok(GuardrailInfo {
        shipped: gantry_core::guardrail::shipped().to_vec(),
        problems: Guardrails::compile(&settings.guardrails)
            .problems()
            .to_vec(),
    })
}

#[tauri::command]
#[specta::specta]
pub fn get_secret_store_status(state: State<'_, AppState>) -> Result<SecretStoreStatus, ErrorDto> {
    Ok(state.secrets.status().clone())
}

/// What Settings → Data & privacy shows and what its buttons do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct DataInfo {
    pub data_dir: String,
    pub database_path: String,
    /// Size of the database file and its WAL, in bytes.
    #[specta(type = specta_typescript::Number)]
    pub database_bytes: u64,
    pub chat_count: u32,
}

#[tauri::command]
#[specta::specta]
pub fn get_data_info(state: State<'_, AppState>) -> Result<DataInfo, ErrorDto> {
    let db = state.store.path().to_path_buf();
    let mut bytes = 0;
    for p in [db.clone(), db.with_extension("db-wal")] {
        if let Ok(m) = std::fs::metadata(&p) {
            bytes += m.len();
        }
    }
    let chat_count = state
        .store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM chats", [], |r| r.get::<_, u32>(0))?))
        .map_err(GantryError::from)?;
    Ok(DataInfo {
        data_dir: state.data_dir.to_string_lossy().into_owned(),
        database_path: db.to_string_lossy().into_owned(),
        database_bytes: bytes,
        chat_count,
    })
}

/// Opens the data directory in the system file manager.
#[tauri::command]
#[specta::specta]
pub fn open_data_dir(app: AppHandle, state: State<'_, AppState>) -> Result<(), ErrorDto> {
    app.opener()
        .open_path(state.data_dir.to_string_lossy(), None::<&str>)
        .map_err(|e| GantryError::internal(e.to_string()))?;
    Ok(())
}

/// A consistent copy of the database without credentials (06 §5, 11 §2).
#[tauri::command]
#[specta::specta]
pub async fn backup_database(state: State<'_, AppState>, path: String) -> Result<(), ErrorDto> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || store.backup_to(std::path::Path::new(&path)))
        .await
        .map_err(|e| GantryError::internal(e.to_string()))?
        .map_err(GantryError::from)?;
    Ok(())
}

/// `PRAGMA integrity_check` then `VACUUM`; never automatic (06 §6).
#[tauri::command]
#[specta::specta]
pub async fn maintain_database(state: State<'_, AppState>) -> Result<(), ErrorDto> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.integrity_check()?;
        store.vacuum()
    })
    .await
    .map_err(|e| GantryError::internal(e.to_string()))?
    .map_err(GantryError::from)?;
    Ok(())
}
