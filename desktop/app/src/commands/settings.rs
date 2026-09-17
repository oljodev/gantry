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

/// One decision the guard made, as Settings → Guard lists it (04 §6). The chat's title comes
/// with it so a row can say where the decision happened; the page links back to it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct GuardDecision {
    pub call_id: gantry_core::CallId,
    pub chat_id: gantry_core::ChatId,
    pub chat_title: String,
    pub connector_name: String,
    pub tool: String,
    pub tier: gantry_core::RiskTier,
    pub summary: String,
    pub verdict: gantry_core::JudgeVerdict,
    #[specta(type = specta_typescript::Number)]
    pub at: i64,
}

/// The guard's last decisions, newest first (04 §6, §11).
#[tauri::command]
#[specta::specta]
pub fn list_guard_decisions(
    state: State<'_, AppState>,
    limit: u32,
) -> Result<Vec<GuardDecision>, ErrorDto> {
    let limit = limit.clamp(1, 200);
    let rows = state
        .store
        .read(move |conn| {
            let calls = gantry_store::repos::tool_calls::recent_judged(conn, limit)?;
            let mut out = Vec::with_capacity(calls.len());
            for call in calls {
                let title = gantry_store::repos::chats::get(conn, call.chat_id)?
                    .map(|c| c.title)
                    .unwrap_or_default();
                out.push((call, title));
            }
            Ok(out)
        })
        .map_err(|e| GantryError::Store(e.to_string()))?;
    Ok(rows
        .into_iter()
        .filter_map(|(call, chat_title)| {
            Some(GuardDecision {
                call_id: call.id,
                chat_id: call.chat_id,
                chat_title,
                connector_name: call.connector_name,
                tool: call.tool,
                tier: call.tier,
                summary: call.display.summary,
                verdict: call.judge?,
                at: call.started_at.or(call.ended_at).unwrap_or_default(),
            })
        })
        .collect())
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
    /// Conversations the user had. Sub agents are chat rows too (18 A1) and are counted apart,
    /// because "412 chats" on a machine with forty of them would be a number about the schema.
    pub chat_count: u32,
    /// Sub-agent transcripts kept (18 §9). They go when the chat that started them goes, and
    /// sooner if a retention is set.
    pub sub_agent_count: u32,
    /// Files under `blobs/`: attachments, artifact versions, the edit journal's before and
    /// after, project knowledge (06 §1).
    pub blob_count: u32,
    #[specta(type = specta_typescript::Number)]
    pub blob_bytes: u64,
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
    let (chat_count, sub_agent_count) = state
        .store
        .read(|c| {
            Ok((
                c.query_row(
                    "SELECT count(*) FROM chats WHERE parent_turn_id IS NULL",
                    [],
                    |r| r.get::<_, u32>(0),
                )?,
                c.query_row(
                    "SELECT count(*) FROM chats WHERE parent_turn_id IS NOT NULL",
                    [],
                    |r| r.get::<_, u32>(0),
                )?,
            ))
        })
        .map_err(GantryError::from)?;
    let (blob_count, blob_bytes) = blob_usage(state.blobs.root());
    Ok(DataInfo {
        data_dir: state.data_dir.to_string_lossy().into_owned(),
        database_path: db.to_string_lossy().into_owned(),
        database_bytes: bytes,
        chat_count,
        sub_agent_count,
        blob_count,
        blob_bytes,
    })
}

/// How many files are under `blobs/` and what they come to. Counted from the directory rather
/// than from the `blobs` table, because the directory is the thing taking up the disk.
fn blob_usage(root: &std::path::Path) -> (u32, u64) {
    let (mut count, mut bytes) = (0u32, 0u64);
    let Ok(prefixes) = std::fs::read_dir(root) else {
        return (0, 0);
    };
    for prefix in prefixes.flatten() {
        let Ok(files) = std::fs::read_dir(prefix.path()) else {
            continue;
        };
        for file in files.flatten() {
            if let Ok(m) = file.metadata()
                && m.is_file()
            {
                count += 1;
                bytes += m.len();
            }
        }
    }
    (count, bytes)
}

/// What a sweep removed, for the toast that reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct BlobSweep {
    pub files: u32,
    #[specta(type = specta_typescript::Number)]
    pub bytes: u64,
}

/// Deletes the blobs nothing references any more (06 §3, §8). Runs weekly by itself; this is the
/// "and on demand" half, for someone who has just deleted a great deal and wants the disk back.
#[tauri::command]
#[specta::specta]
pub async fn sweep_blobs(state: State<'_, AppState>) -> Result<BlobSweep, ErrorDto> {
    let store = state.store.clone();
    let blobs = state.blobs.clone();
    let report = tauri::async_runtime::spawn_blocking(move || {
        store.write_blocking(move |conn| gantry_store::sweep::run(conn, &blobs))
    })
    .await
    .map_err(|e| GantryError::internal(e.to_string()))?
    .map_err(GantryError::from)?;
    Ok(BlobSweep {
        files: u32::try_from(report.files).unwrap_or(u32::MAX),
        bytes: report.bytes,
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
