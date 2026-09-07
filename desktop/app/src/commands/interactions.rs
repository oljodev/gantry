//! Pending decisions (docs/plan/04 §10) and the activity detail (05 §1).

use gantry_core::{
    CallId, ChatId, ErrorDto, GantryError, Interaction, InteractionId, InteractionResolution,
    ToolCallDto,
};
use gantry_store::repos;
use tauri::{AppHandle, State};
use tauri_specta::Event;

use crate::{AppState, events::InteractionsChanged};

/// Interactions waiting for the user, oldest first, for one chat or every chat. Cards render
/// from the run store while a turn streams; this fills a chat view that mounts later.
#[tauri::command]
#[specta::specta]
pub fn list_pending_interactions(
    state: State<'_, AppState>,
    chat_id: Option<ChatId>,
) -> Result<Vec<Interaction>, ErrorDto> {
    Ok(state.turns.interactions().list_pending(chat_id))
}

/// Answers a pending decision; the waiting turn continues.
#[tauri::command]
#[specta::specta]
pub fn resolve_interaction(
    app: AppHandle,
    state: State<'_, AppState>,
    interaction_id: InteractionId,
    resolution: InteractionResolution,
) -> Result<Interaction, ErrorDto> {
    let resolved = state
        .turns
        .resolve_interaction(interaction_id, resolution)?;
    let _ = InteractionsChanged {
        chat_id: resolved.chat_id,
        pending: state.turns.interactions().pending_count(resolved.chat_id),
    }
    .emit(&app);
    Ok(resolved)
}

/// One tool call with its full result, for the detail pane of a finished turn.
#[tauri::command]
#[specta::specta]
pub fn get_tool_call(state: State<'_, AppState>, call_id: CallId) -> Result<ToolCallDto, ErrorDto> {
    let call = state
        .store
        .read(|conn| repos::tool_calls::get(conn, &call_id))
        .map_err(GantryError::from)?
        .ok_or_else(|| GantryError::not_found(format!("tool call {call_id}")))?;
    let detail = state
        .turns
        .chats()
        .get(call.chat_id)?
        .ok_or_else(|| GantryError::not_found(format!("chat {}", call.chat_id)))?;
    Ok(detail
        .turns
        .into_iter()
        .flat_map(|t| t.tool_calls)
        .find(|c| c.id == call_id)
        .unwrap_or(call))
}
