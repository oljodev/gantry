use std::sync::Arc;

use gantry_agent::EventSink;
use gantry_core::{
    AgentEventBatch, AttachmentInput, CallId, ChatId, ErrorDto, GantryError, TurnId,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State, ipc::Channel};
use tauri_specta::Event;

use crate::{AppState, events::ChatsChanged};

/// Delivers batches to one webview channel; sends fail silently once the view is gone (05 §5).
pub struct ChannelSink(Channel<AgentEventBatch>);

impl EventSink for ChannelSink {
    fn emit(&self, batch: AgentEventBatch) {
        if let Err(err) = self.0.send(batch) {
            log::debug!("channel send failed: {err}");
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ActiveTurn {
    pub chat_id: ChatId,
    pub turn_id: TurnId,
}

/// Starts a turn and returns at once; the channel carries the turn's events until it ends.
/// Attachments are read and stored before anything is sent; a bad one fails the whole call.
/// `skills` are the ones the composer's `/name` forced for this message (12 §A4 rule 5).
///
/// Async, and the start itself on a blocking thread, because of those attachments: reading them
/// is file IO and a document is parsed before it is text, which for a long PDF is seconds. A
/// synchronous command would spend them on the thread the window is drawn on.
#[tauri::command]
#[specta::specta]
pub async fn send_message(
    app: AppHandle,
    state: State<'_, AppState>,
    chat_id: ChatId,
    text: String,
    attachments: Vec<AttachmentInput>,
    skills: Vec<String>,
    on_event: Channel<AgentEventBatch>,
) -> Result<TurnId, ErrorDto> {
    let turns = state.turns.clone();
    let turn = tokio::task::spawn_blocking(move || {
        turns.start(
            chat_id,
            text,
            attachments,
            skills,
            Arc::new(ChannelSink(on_event)),
        )
    })
    .await
    .map_err(|_| GantryError::internal("sending the message was interrupted"))??;
    let _ = ChatsChanged {
        chat_ids: vec![chat_id],
    }
    .emit(&app);
    Ok(turn)
}

/// Drops the chat's last turn and sends its user message again over a fresh channel.
#[tauri::command]
#[specta::specta]
pub fn retry_turn(
    app: AppHandle,
    state: State<'_, AppState>,
    chat_id: ChatId,
    turn_id: TurnId,
    on_event: Channel<AgentEventBatch>,
) -> Result<TurnId, ErrorDto> {
    let turn = state
        .turns
        .retry(chat_id, turn_id, Arc::new(ChannelSink(on_event)))?;
    let _ = ChatsChanged {
        chat_ids: vec![chat_id],
    }
    .emit(&app);
    Ok(turn)
}

/// **Allow anyway** on a call the guard blocked (04 §6): the override is remembered and a new
/// turn starts, telling the model to make the call again.
#[tauri::command]
#[specta::specta]
pub fn allow_blocked_call(
    app: AppHandle,
    state: State<'_, AppState>,
    chat_id: ChatId,
    call_id: CallId,
    on_event: Channel<AgentEventBatch>,
) -> Result<TurnId, ErrorDto> {
    let turn = state
        .turns
        .allow_blocked(chat_id, call_id, Arc::new(ChannelSink(on_event)))?;
    let _ = ChatsChanged {
        chat_ids: vec![chat_id],
    }
    .emit(&app);
    Ok(turn)
}

/// Marks a guard decision right or wrong, or takes the mark back (04 §6). Stored with the
/// decision for later prompt tuning; nothing reads it yet.
#[tauri::command]
#[specta::specta]
pub fn mark_judge_decision(
    state: State<'_, AppState>,
    call_id: CallId,
    wrong: Option<bool>,
) -> Result<(), ErrorDto> {
    state.turns.mark_verdict(&call_id, wrong)?;
    Ok(())
}

/// Whether the turn was running.
#[tauri::command]
#[specta::specta]
pub fn cancel_turn(state: State<'_, AppState>, turn_id: TurnId) -> Result<bool, ErrorDto> {
    Ok(state.turns.cancel(turn_id))
}

/// Reattaches to a running turn: one snapshot, then live batches (05 §3).
#[tauri::command]
#[specta::specta]
pub fn subscribe_turn(
    state: State<'_, AppState>,
    turn_id: TurnId,
    since_seq: u32,
    on_event: Channel<AgentEventBatch>,
) -> Result<(), ErrorDto> {
    state
        .turns
        .subscribe(turn_id, since_seq, Arc::new(ChannelSink(on_event)))?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn list_active_turns(state: State<'_, AppState>) -> Result<Vec<ActiveTurn>, ErrorDto> {
    Ok(state
        .turns
        .list_active()
        .into_iter()
        .map(|(chat_id, turn_id)| ActiveTurn { chat_id, turn_id })
        .collect())
}
