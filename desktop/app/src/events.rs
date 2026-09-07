//! Global invalidation events (docs/plan/01 §4): ids only, never data.

use gantry_core::{ArtifactId, ChatId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct ChatsChanged {
    pub chat_ids: Vec<ChatId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct ProvidersChanged;

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct SettingsChanged;

/// A chat's count of decisions waiting for the user changed (04 §10: sidebar badges).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct InteractionsChanged {
    pub chat_id: ChatId,
    pub pending: u32,
}

/// A user edit or restore made a new artifact version outside a turn (13 §10).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct ArtifactsChanged {
    pub chat_id: ChatId,
    pub artifact_id: ArtifactId,
}

/// The installed connectors, their auth state or their attachment to a chat changed (03 §10).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct ConnectorsChanged;
