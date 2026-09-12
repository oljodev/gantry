use gantry_agent::{ChatPatch, ExportFormat};
use gantry_core::{
    ChatDetail, ChatId, ChatSummary, ErrorDto, Feedback, GantryError, Mode, ModelRef,
    ReasoningEffort, SearchHit, Surface, TurnId,
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

/// A new session. `surface` is `chat` unless given; a code session must arrive with the folder
/// it will work in (docs/plan/16 C5).
#[tauri::command]
#[specta::specta]
pub fn create_chat(
    app: AppHandle,
    state: State<'_, AppState>,
    model: Option<ModelRef>,
    surface: Option<Surface>,
    roots: Option<Vec<String>>,
) -> Result<ChatSummary, ErrorDto> {
    let chat = state.turns.create_session(
        surface.unwrap_or_default(),
        roots.unwrap_or_default(),
        model,
        false,
    )?;
    let _ = ChatsChanged {
        chat_ids: vec![chat.id],
    }
    .emit(&app);
    Ok(chat)
}

/// One surface's sessions. The two lists never mix (16 §6).
#[tauri::command]
#[specta::specta]
pub fn list_chats(
    state: State<'_, AppState>,
    surface: Option<Surface>,
) -> Result<Vec<ChatSummary>, ErrorDto> {
    Ok(state.turns.chats().list(surface.unwrap_or_default())?)
}

/// Adds a folder to a session (16 §7). The path is taken as the user picked it; the workspace
/// layer canonicalises it when the file tools arrive.
#[tauri::command]
#[specta::specta]
pub fn add_chat_root(
    app: AppHandle,
    state: State<'_, AppState>,
    chat_id: ChatId,
    path: String,
) -> Result<Vec<String>, ErrorDto> {
    let roots = state.turns.chats().add_root(chat_id, path)?;
    let _ = ChatsChanged {
        chat_ids: vec![chat_id],
    }
    .emit(&app);
    Ok(roots)
}

/// Removes a folder. A code session may not drop its last one: it would stop being one.
#[tauri::command]
#[specta::specta]
pub fn remove_chat_root(
    app: AppHandle,
    state: State<'_, AppState>,
    chat_id: ChatId,
    path: String,
) -> Result<Vec<String>, ErrorDto> {
    let chats = state.turns.chats();
    let detail = chats
        .get(chat_id)?
        .ok_or_else(|| GantryError::not_found(format!("chat {chat_id}")))?;
    if detail.surface.needs_folder() && detail.roots.len() <= 1 {
        return Err(GantryError::invalid("a code session needs a folder").into());
    }
    let roots = chats.remove_root(chat_id, path)?;
    let _ = ChatsChanged {
        chat_ids: vec![chat_id],
    }
    .emit(&app);
    Ok(roots)
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

/// A `data:` URL for an image already in the transcript, so a sent message can show the
/// picture rather than a file name. Only image types, only inside the size cap.
#[tauri::command]
#[specta::specta]
pub fn blob_image(state: State<'_, AppState>, hash: String, mime: String) -> Option<String> {
    use base64::Engine;
    if !matches!(
        mime.as_str(),
        "image/png" | "image/jpeg" | "image/gif" | "image/webp"
    ) {
        return None;
    }
    let bytes = state.blobs.get(&hash).ok()?;
    if bytes.is_empty() || bytes.len() > gantry_core::MAX_IMAGE_BYTES {
        return None;
    }
    let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Some(format!("data:{mime};base64,{data}"))
}

/// A `data:` URL for a picture, a sound file or a clip the model produced. The bytes live in
/// the blob store rather than in the transcript (`Chats::append_turn_message`), so a reopened
/// chat has to ask for them; the live answer never comes through here.
#[tauri::command]
#[specta::specta]
pub fn blob_media(state: State<'_, AppState>, hash: String, mime: String) -> Option<String> {
    use base64::Engine;
    let kind = mime.split('/').next().unwrap_or_default();
    if !matches!(kind, "image" | "audio" | "video") {
        return None;
    }
    let bytes = state.blobs.get(&hash).ok()?;
    if bytes.is_empty() || bytes.len() > gantry_providers::MAX_MEDIA_BYTES {
        return None;
    }
    let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Some(format!("data:{mime};base64,{data}"))
}

/// A `data:` URL for an image on disk, so the composer can show what is attached before the
/// message is sent. Anything that is not a supported image, or is over the image cap, answers
/// with nothing rather than an error: a preview is a convenience, not a promise.
#[tauri::command]
#[specta::specta]
pub fn image_preview(path: String) -> Option<String> {
    use base64::Engine;
    let mime = match std::path::Path::new(&path)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => return None,
    };
    let bytes = std::fs::read(&path).ok()?;
    if bytes.is_empty() || bytes.len() > gantry_core::MAX_IMAGE_BYTES {
        return None;
    }
    let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Some(format!("data:{mime};base64,{data}"))
}

/// Opens an incognito chat in its own window (docs/plan/15 A21).
///
/// The chat is created here rather than by the frontend, and the window is given its id, so the
/// window owns the session from the first frame: the close handler below has something to
/// delete however the window ends, and no path exists where a webview creates an incognito chat
/// and then fails to claim it.
///
/// A separate window rather than a mode inside the main one, for the same reason the artifact
/// windows are separate: a private conversation that shares a sidebar with the history is one
/// keystroke from being in it, and "which window am I in" is a question a person can answer at
/// a glance.
#[tauri::command]
#[specta::specta]
pub fn open_incognito_window(
    app: AppHandle,
    state: State<'_, AppState>,
    model: Option<ModelRef>,
) -> Result<ChatSummary, ErrorDto> {
    use tauri::{Manager, WindowEvent};

    let chat = state
        .turns
        .create_session(Surface::Chat, Vec::new(), model, true)?;
    let label = format!("incognito-{}", chat.id);
    let url = tauri::WebviewUrl::App(format!("index.html#/incognito?chat={}", chat.id).into());
    let builder = tauri::WebviewWindowBuilder::new(&app, &label, url)
        .title("Incognito chat")
        .inner_size(1000.0, 760.0)
        .min_inner_size(480.0, 400.0);
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    #[cfg(not(target_os = "macos"))]
    let builder = builder.decorations(false);
    let window = builder.build().map_err(|e| {
        // The chat would otherwise sit in the table until the next startup sweep.
        let _ = state.turns.chats().delete(chat.id);
        GantryError::invalid(format!("could not open the window: {e}"))
    })?;

    // The session ends with the window, whichever way the window ends. `Destroyed` fires for a
    // close, a quit and a kill of the webview alike; the startup sweep covers the one case it
    // cannot, which is the process not living long enough to run this.
    let handle = app.clone();
    let chat_id = chat.id;
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed)
            && let Some(state) = handle.try_state::<AppState>()
        {
            match state.turns.chats().delete(chat_id) {
                Ok(_) => log::info!("incognito session closed and deleted"),
                Err(err) => log::warn!("could not delete the incognito session: {err}"),
            }
            let _ = ChatsChanged {
                chat_ids: vec![chat_id],
            }
            .emit(&handle);
        }
    });
    Ok(chat)
}
