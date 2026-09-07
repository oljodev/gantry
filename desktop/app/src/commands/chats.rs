use gantry_agent::ChatPatch;
use gantry_core::{
    ChatDetail, ChatId, ChatSummary, ErrorDto, GantryError, Mode, ModelRef, ReasoningEffort,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_specta::Event;

use crate::{AppState, events::ChatsChanged};

/// Fields the composer and the sidebar change; absent fields stay as they are.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct ChatUpdate {
    pub model: Option<ModelRef>,
    pub mode: Option<Mode>,
    pub guard: Option<bool>,
    pub effort: Option<ReasoningEffort>,
    pub title: Option<String>,
    pub pinned: Option<bool>,
}

#[tauri::command]
#[specta::specta]
pub fn create_chat(
    app: AppHandle,
    state: State<'_, AppState>,
    model: Option<ModelRef>,
) -> Result<ChatSummary, ErrorDto> {
    let chat = state.turns.create_chat(model);
    let _ = ChatsChanged {
        chat_ids: vec![chat.id],
    }
    .emit(&app);
    Ok(chat)
}

#[tauri::command]
#[specta::specta]
pub fn list_chats(state: State<'_, AppState>) -> Result<Vec<ChatSummary>, ErrorDto> {
    Ok(state.turns.chats().list())
}

#[tauri::command]
#[specta::specta]
pub fn get_chat(state: State<'_, AppState>, chat_id: ChatId) -> Result<ChatDetail, ErrorDto> {
    state
        .turns
        .chats()
        .get(chat_id)
        .ok_or_else(|| GantryError::not_found(format!("chat {chat_id}")).into())
}

#[tauri::command]
#[specta::specta]
pub fn update_chat(
    app: AppHandle,
    state: State<'_, AppState>,
    chat_id: ChatId,
    update: ChatUpdate,
) -> Result<ChatSummary, ErrorDto> {
    let summary = state.turns.chats().update(
        chat_id,
        ChatPatch {
            model: update.model,
            mode: update.mode,
            guard: update.guard,
            effort: update.effort,
            title: update
                .title
                .map(|t| t.trim().to_owned())
                .filter(|t| !t.is_empty()),
            pinned: update.pinned,
        },
    )?;
    let _ = ChatsChanged {
        chat_ids: vec![chat_id],
    }
    .emit(&app);
    Ok(summary)
}

#[tauri::command]
#[specta::specta]
pub fn delete_chat(
    app: AppHandle,
    state: State<'_, AppState>,
    chat_id: ChatId,
) -> Result<(), ErrorDto> {
    if let Some(turn) = state.turns.active_turn_for(chat_id) {
        state.turns.cancel(turn);
    }
    if !state.turns.chats().delete(chat_id) {
        return Err(GantryError::not_found(format!("chat {chat_id}")).into());
    }
    let _ = ChatsChanged {
        chat_ids: vec![chat_id],
    }
    .emit(&app);
    Ok(())
}
