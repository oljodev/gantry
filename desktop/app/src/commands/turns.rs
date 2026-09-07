use std::sync::Arc;

use gantry_agent::EventSink;
use gantry_core::{AgentEventBatch, ChatId, ErrorDto, TurnId};
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
#[tauri::command]
#[specta::specta]
pub fn send_message(
    app: AppHandle,
    state: State<'_, AppState>,
    chat_id: ChatId,
    text: String,
    on_event: Channel<AgentEventBatch>,
) -> Result<TurnId, ErrorDto> {
    let turn = state
        .turns
        .start(chat_id, text, Arc::new(ChannelSink(on_event)))?;
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
