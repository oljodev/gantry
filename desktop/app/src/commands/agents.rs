//! The Sub agents section of Customize (docs/plan/18 §3, §10 phase B, 11 §2).
//!
//! The library is a table, so these are four ordinary writes. The settings beside it — who
//! answers a card, the limits, the model rules — are part of `Settings` and go through
//! `update_settings` like everything else: a page of switches that saved through two different
//! mechanisms would be a page with two ways to fail.

use gantry_core::{AgentType, ErrorDto, GantryError};
use gantry_store::repos;
use tauri::{AppHandle, State};
use tauri_specta::Event;

use crate::{AppState, events::AgentTypesChanged};

/// The whole library, by name, switched-off types included.
#[tauri::command]
#[specta::specta]
pub fn list_agent_types(state: State<'_, AppState>) -> Result<Vec<AgentType>, ErrorDto> {
    Ok(state
        .store
        .read(repos::agents::list)
        .map_err(GantryError::from)?)
}

/// Creates a type or replaces every field of one.
///
/// The id is the name the model writes in a call, so it is fixed at creation: renaming a type
/// would leave a `subagents__run` in some transcript naming something that no longer exists.
#[tauri::command]
#[specta::specta]
pub async fn save_agent_type(
    app: AppHandle,
    state: State<'_, AppState>,
    agent: AgentType,
) -> Result<(), ErrorDto> {
    let id = agent.id.trim().to_owned();
    gantry_agent::subagents::library::validate(&agent).map_err(GantryError::invalid)?;
    // A built-in stays a built-in however it is edited: the flag is what Reset and the refusal
    // to delete both read, and letting the form send it would make either of them a lie.
    let builtin = state
        .store
        .read({
            let id = id.clone();
            move |c| repos::agents::get(c, &id)
        })
        .map_err(GantryError::from)?
        .is_some_and(|existing| existing.builtin);
    let agent = AgentType {
        id,
        builtin,
        ..agent
    };
    state
        .store
        .write(move |c| repos::agents::upsert(c, &agent))
        .await
        .map_err(GantryError::from)?;
    let _ = AgentTypesChanged.emit(&app);
    Ok(())
}

/// Deletes a type the user wrote. A built-in is switched off instead, which the UI offers
/// rather than a bin that refuses.
#[tauri::command]
#[specta::specta]
pub async fn delete_agent_type(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), ErrorDto> {
    let found = state
        .store
        .read({
            let id = id.clone();
            move |c| repos::agents::get(c, &id)
        })
        .map_err(GantryError::from)?;
    if found.is_some_and(|a| a.builtin) {
        return Err(GantryError::invalid(
            "this sub agent ships with Gantry and cannot be deleted; switch it off instead",
        )
        .into());
    }
    state
        .store
        .write(move |c| repos::agents::remove(c, &id))
        .await
        .map_err(GantryError::from)?;
    let _ = AgentTypesChanged.emit(&app);
    Ok(())
}

/// Puts a built-in back the way it shipped, keeping whether it is switched on.
///
/// Switching it back on would be the surprising half: somebody asking for the original
/// instructions has not asked for the type to start being offered again.
#[tauri::command]
#[specta::specta]
pub async fn reset_agent_type(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<AgentType, ErrorDto> {
    let original = gantry_agent::subagents::original(&id)
        .ok_or_else(|| GantryError::not_found(format!("{id} does not ship with Gantry")))?;
    let enabled = state
        .store
        .read({
            let id = id.clone();
            move |c| repos::agents::get(c, &id)
        })
        .map_err(GantryError::from)?
        .is_none_or(|a| a.enabled);
    let restored = AgentType {
        enabled,
        ..original
    };
    state
        .store
        .write({
            let restored = restored.clone();
            move |c| repos::agents::upsert(c, &restored)
        })
        .await
        .map_err(GantryError::from)?;
    let _ = AgentTypesChanged.emit(&app);
    Ok(restored)
}

/// The switch on the row. Off means the model is not offered the type at all, which is a
/// shorter tool description as well as one fewer thing it can do.
#[tauri::command]
#[specta::specta]
pub async fn set_agent_type_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<(), ErrorDto> {
    let found = state
        .store
        .read({
            let id = id.clone();
            move |c| repos::agents::get(c, &id)
        })
        .map_err(GantryError::from)?
        .ok_or_else(|| GantryError::not_found(format!("sub agent {id}")))?;
    let agent = AgentType { enabled, ..found };
    state
        .store
        .write(move |c| repos::agents::upsert(c, &agent))
        .await
        .map_err(GantryError::from)?;
    let _ = AgentTypesChanged.emit(&app);
    Ok(())
}
