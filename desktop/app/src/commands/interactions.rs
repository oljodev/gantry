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
    events::{ChatsChanged, ConnectorsChanged, InteractionsChanged, MemoryChanged, SkillsChanged},
};

/// What the model is told about a proposal it made, on its next turn. `None` for every other
/// kind of interaction, which the turn waited for and already knows the answer to.
fn proposal_note(resolved: &Interaction) -> Option<String> {
    use gantry_core::{MemoryProposalOutcome, SkillProposalOutcome};
    match resolved.resolution.as_ref()? {
        InteractionResolution::SkillProposal { outcome } => Some(match outcome {
            SkillProposalOutcome::Saved { name, .. } => format!(
                "The user kept your skill proposal as `{name}`. It is available from now on; do \
                 not offer it again."
            ),
            SkillProposalOutcome::Discarded => {
                "The user discarded your skill proposal. Do not offer it again in this \
                 conversation."
                    .to_owned()
            }
        }),
        InteractionResolution::MemoryProposal { outcome } => Some(match outcome {
            MemoryProposalOutcome::Saved { .. } => {
                "The user kept the memory you proposed; they may have edited the wording. It \
                 will be in your context in later chats."
                    .to_owned()
            }
            MemoryProposalOutcome::Forgotten { .. } => {
                "The user agreed to forget that memory. Stop relying on it.".to_owned()
            }
            MemoryProposalOutcome::Discarded => {
                "The user did not keep that memory. Do not propose it again, and do not act as \
                 though it were remembered."
                    .to_owned()
            }
        }),
        _ => None,
    }
}

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
    // An answered access request or suggestion attaches a connector inside the waiting turn, so
    // the chat's connector list is stale the moment this returns (03 §9, 04 §9). Named rather
    // than inferred from "not a permission", so a later kind that changes nothing — an
    // elicitation does not — is not swept in by a negation nobody revisits.
    let widens = matches!(
        resolution,
        InteractionResolution::AccessRequest { .. }
            | InteractionResolution::ConnectorSuggestion { .. }
    );
    let resolved = state
        .turns
        .resolve_interaction(interaction_id, resolution)?;
    if widens {
        let _ = ConnectorsChanged.emit(&app);
    }
    // A proposal did not block the turn (12 §A5, §B3), so the model never saw an answer. It is
    // told what happened the way every other out-of-band change is told: a `SystemNote` before
    // the next user message (10 §4). Saying nothing would leave it believing it had remembered
    // something.
    if let Some(note) = proposal_note(&resolved) {
        state
            .turns
            .chats()
            .append_system_note(resolved.chat_id, note)?;
        let _ = SkillsChanged.emit(&app);
        let _ = MemoryChanged.emit(&app);
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
