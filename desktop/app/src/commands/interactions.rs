//! Pending decisions (docs/plan/04 §10) and the activity detail (05 §1).

use gantry_core::{
    CallId, ChatGrant, ChatId, ErrorDto, GantryError, GrantId, Interaction, InteractionId,
    InteractionResolution, ToolCallDto,
};
use gantry_store::repos;
use tauri::{AppHandle, State};
use tauri_specta::Event;

use crate::{
    AppState,
    events::{ChatsChanged, ConnectorsChanged, InteractionsChanged},
};

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
    // An answered access request or suggestion attaches a connector inside the waiting turn,
    // so the chat's connector list is stale the moment this returns (03 §9, 04 §9).
    let widens = !matches!(resolution, InteractionResolution::Permission { .. });
    let resolved = state
        .turns
        .resolve_interaction(interaction_id, resolution)?;
    if widens {
        let _ = ConnectorsChanged.emit(&app);
    }
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

/// The chat's standing permissions (04 §8), oldest first, for the Permissions panel.
#[tauri::command]
#[specta::specta]
pub fn list_chat_grants(
    state: State<'_, AppState>,
    chat_id: ChatId,
) -> Result<Vec<ChatGrant>, ErrorDto> {
    Ok(state.turns.chats().grants(chat_id)?)
}

/// Revokes one standing permission. The next call it would have covered asks again.
#[tauri::command]
#[specta::specta]
pub fn revoke_chat_grant(
    app: AppHandle,
    state: State<'_, AppState>,
    grant_id: GrantId,
) -> Result<(), ErrorDto> {
    if let Some(chat_id) = state.turns.chats().revoke_grant(grant_id)? {
        let _ = ChatsChanged {
            chat_ids: vec![chat_id],
        }
        .emit(&app);
    }
    Ok(())
}

/// Revokes every standing permission of one chat.
#[tauri::command]
#[specta::specta]
pub fn revoke_all_chat_grants(
    app: AppHandle,
    state: State<'_, AppState>,
    chat_id: ChatId,
) -> Result<u32, ErrorDto> {
    let n = state.turns.chats().revoke_all_grants(chat_id)?;
    let _ = ChatsChanged {
        chat_ids: vec![chat_id],
    }
    .emit(&app);
    Ok(u32::try_from(n).unwrap_or(u32::MAX))
}
