use gantry_agent::{ChatPatch, ExportFormat};
use gantry_core::{
    ChatDetail, ChatId, ChatSummary, ErrorDto, Feedback, GantryError, Mode, ModelRef,
    ReasoningEffort, SearchHit, TurnId,
};
use gantry_store::repos;
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
    pub web_search: Option<bool>,
    pub title: Option<String>,
    pub pinned: Option<bool>,
    pub archived: Option<bool>,
    /// Chat-level custom instructions (10 §2, layer 6).
    pub instructions: Option<String>,
}

/// What developer mode shows: the frozen prompt and the notes appended since (10 §4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct SystemPromptView {
    pub snapshot: String,
    pub notes: Vec<String>,
}

#[tauri::command]
#[specta::specta]
pub fn create_chat(
    app: AppHandle,
    state: State<'_, AppState>,
    model: Option<ModelRef>,
) -> Result<ChatSummary, ErrorDto> {
    let chat = state.turns.create_chat(model)?;
    let _ = ChatsChanged {
        chat_ids: vec![chat.id],
    }
    .emit(&app);
    Ok(chat)
}

#[tauri::command]
#[specta::specta]
pub fn list_chats(state: State<'_, AppState>) -> Result<Vec<ChatSummary>, ErrorDto> {
    Ok(state.turns.chats().list()?)
}

#[tauri::command]
#[specta::specta]
pub fn get_chat(state: State<'_, AppState>, chat_id: ChatId) -> Result<ChatDetail, ErrorDto> {
    state
        .turns
        .chats()
        .get(chat_id)?
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
    let summary = state.turns.update_chat(
        chat_id,
        ChatPatch {
            model: update.model,
            mode: update.mode,
            guard: update.guard,
            effort: update.effort,
            web_search: update.web_search,
            title: update
                .title
                .map(|t| t.trim().to_owned())
                .filter(|t| !t.is_empty()),
            pinned: update.pinned,
            archived: update.archived,
            instructions: update.instructions.map(|i| i.trim().to_owned()),
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
    if !state.turns.chats().delete(chat_id)? {
        return Err(GantryError::not_found(format!("chat {chat_id}")).into());
    }
    let _ = ChatsChanged {
        chat_ids: vec![chat_id],
    }
    .emit(&app);
    Ok(())
}

/// The user's verdict on a reply; `None` clears it.
#[tauri::command]
#[specta::specta]
pub fn rate_turn(
    app: AppHandle,
    state: State<'_, AppState>,
    chat_id: ChatId,
    turn_id: TurnId,
    feedback: Option<Feedback>,
) -> Result<(), ErrorDto> {
    state.turns.chats().rate_turn(chat_id, turn_id, feedback)?;
    let _ = ChatsChanged {
        chat_ids: vec![chat_id],
    }
    .emit(&app);
    Ok(())
}

/// Chats by title and messages by text (15 A15).
#[tauri::command]
#[specta::specta]
pub fn search(
    state: State<'_, AppState>,
    query: String,
    limit: u32,
) -> Result<Vec<SearchHit>, ErrorDto> {
    let limit = limit.clamp(1, 50);
    Ok(state
        .store
        .read(|c| repos::search::search(c, &query, limit))
        .map_err(GantryError::from)?)
}

/// The assembled system prompt of a chat, read-only (11 §2, Advanced → developer mode).
#[tauri::command]
#[specta::specta]
pub fn get_system_prompt(
    state: State<'_, AppState>,
    chat_id: ChatId,
) -> Result<SystemPromptView, ErrorDto> {
    let (snapshot, notes) = state
        .turns
        .chats()
        .system_prompt(chat_id)?
        .ok_or_else(|| GantryError::not_found(format!("chat {chat_id}")))?;
    Ok(SystemPromptView { snapshot, notes })
}

/// Writes the chat to `path` as Markdown or JSON (11 §2, Data & privacy).
#[tauri::command]
#[specta::specta]
pub fn export_chat(
    state: State<'_, AppState>,
    chat_id: ChatId,
    format: ExportFormat,
    path: String,
) -> Result<(), ErrorDto> {
    let chat = state
        .turns
        .chats()
        .get(chat_id)?
        .ok_or_else(|| GantryError::not_found(format!("chat {chat_id}")))?;
    let body = gantry_agent::export::render(&chat, format);
    std::fs::write(&path, body).map_err(GantryError::Io)?;
    Ok(())
}
