//! The Memory section of Customize (docs/plan/12 §B5, 15 A18).
//!
//! Every row the model ever sees is reachable from here, which is the promise of §B1. The
//! commands are deliberately dull — list, create, update, delete, restore, export, import —
//! because the interesting decisions all happened before a row existed.

use gantry_core::{
    ChatId, ErrorDto, GantryError, MemoryDto, MemoryInput, MemoryKind, MemoryScopeKind,
    MemorySource, MessageId,
};
use gantry_store::repos::memories::MemoryFilter;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_specta::Event;

use crate::{AppState, events::MemoryChanged};

/// The page's filters, as the frontend sends them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct MemoryQuery {
    pub search: String,
    pub scope_kind: Option<MemoryScopeKind>,
    pub kind: Option<MemoryKind>,
    pub source: Option<MemorySource>,
    pub enabled: Option<bool>,
    /// Recently deleted instead of the live set.
    pub archived: bool,
}

#[tauri::command]
#[specta::specta]
pub fn list_memories(
    state: State<'_, AppState>,
    query: MemoryQuery,
) -> Result<Vec<MemoryDto>, ErrorDto> {
    Ok(state.memories.list(
        MemoryFilter {
            scope_kind: query.scope_kind,
            kind: query.kind,
            source: query.source,
            enabled: query.enabled,
            archived: query.archived,
        },
        &query.search,
    )?)
}

/// A new memory, as the page's New, `/remember` and **Remember this** all hand it in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct NewMemory {
    pub text: String,
    pub kind: MemoryKind,
    pub scope_kind: MemoryScopeKind,
    pub source: MemorySource,
    /// Provenance (12 §B1): which chat taught us this, and which message in it.
    pub origin_chat_id: Option<ChatId>,
    pub origin_message_id: Option<MessageId>,
}

/// Writes one entry the user asked for: the page's New, `/remember`, or **Remember this** on a
/// selection.
#[tauri::command]
#[specta::specta]
pub fn create_memory(
    app: AppHandle,
    state: State<'_, AppState>,
    memory: NewMemory,
) -> Result<MemoryDto, ErrorDto> {
    let entry = state.memories.create(
        &memory.text,
        memory.kind,
        memory.scope_kind,
        None,
        memory.source,
        memory.origin_chat_id.map(|c| (c, memory.origin_message_id)),
    )?;
    let _ = MemoryChanged.emit(&app);
    Ok(entry)
}

#[tauri::command]
#[specta::specta]
pub fn update_memory(
    app: AppHandle,
    state: State<'_, AppState>,
    id: gantry_core::MemoryId,
    patch: MemoryInput,
) -> Result<MemoryDto, ErrorDto> {
    let entry = state.memories.update(id, patch)?;
    let _ = MemoryChanged.emit(&app);
    Ok(entry)
}

/// Deletes into Recently deleted, where it stays for thirty days (12 §B5).
#[tauri::command]
#[specta::specta]
pub fn delete_memory(
    app: AppHandle,
    state: State<'_, AppState>,
    id: gantry_core::MemoryId,
) -> Result<(), ErrorDto> {
    state.memories.archive(id)?;
    let _ = MemoryChanged.emit(&app);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn restore_memory(
    app: AppHandle,
    state: State<'_, AppState>,
    id: gantry_core::MemoryId,
) -> Result<(), ErrorDto> {
    state.memories.restore(id)?;
    let _ = MemoryChanged.emit(&app);
    Ok(())
}

/// Removes an entry for good, from Recently deleted. There is nowhere after this.
#[tauri::command]
#[specta::specta]
pub fn forget_memory_for_good(
    app: AppHandle,
    state: State<'_, AppState>,
    id: gantry_core::MemoryId,
) -> Result<(), ErrorDto> {
    state.memories.forget_for_good(id)?;
    let _ = MemoryChanged.emit(&app);
    Ok(())
}

/// Everything, as JSON the user can keep (12 §B5). Plain rows: a memory is a sentence, and an
/// export nobody can read in a text editor would be a worse export.
#[tauri::command]
#[specta::specta]
pub fn export_memories(state: State<'_, AppState>) -> Result<String, ErrorDto> {
    let all = state.memories.list(MemoryFilter::default(), "")?;
    serde_json::to_string_pretty(&all)
        .map_err(|e| GantryError::internal(format!("could not write the export: {e}")).into())
}

/// What an import would do, before it does it: the page reviews it like a skill (12 §B5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct MemoryImport {
    pub entries: Vec<MemoryDto>,
    pub problems: Vec<String>,
}

#[tauri::command]
#[specta::specta]
pub fn review_memory_import(json: String) -> Result<MemoryImport, ErrorDto> {
    let entries: Vec<MemoryDto> = serde_json::from_str(&json).map_err(|e| {
        GantryError::invalid(format!(
            "that is not a Gantry memory export: {e}. Export one from this page to see the shape."
        ))
    })?;
    let problems = entries
        .iter()
        .filter_map(|m| {
            gantry_core::memory::text_problem(&m.text).map(|p| format!("{}: {p}", m.id))
        })
        .collect();
    Ok(MemoryImport { entries, problems })
}

/// Writes a reviewed import. Entries come in as new rows with new ids: an import is a copy,
/// not a merge, and silently overwriting an entry the user has since edited would be the kind
/// of surprise the whole feature is built to avoid.
#[tauri::command]
#[specta::specta]
pub fn import_memories(
    app: AppHandle,
    state: State<'_, AppState>,
    entries: Vec<MemoryDto>,
) -> Result<u32, ErrorDto> {
    let mut written = 0;
    for m in entries {
        if gantry_core::memory::text_problem(&m.text).is_some() {
            continue;
        }
        state.memories.create(
            &m.text,
            m.kind,
            m.scope_kind,
            m.scope_id,
            m.source,
            m.origin_chat_id.map(|c| (c, m.origin_message_id)),
        )?;
        written += 1;
    }
    let _ = MemoryChanged.emit(&app);
    Ok(written)
}
