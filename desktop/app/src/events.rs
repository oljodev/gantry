//! Global invalidation events (docs/plan/01 §4): ids only, never data.

use gantry_core::{ArtifactId, ChatId, InstanceId};
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

/// A sign-in needs the user to type a code on the server's own page (RFC 8628, 03 §7). Sent
/// before the browser opens, and answered by the user, not by the app.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct DeviceCodeNeeded {
    pub instance_id: InstanceId,
    pub connector: String,
    pub user_code: String,
    pub verification_uri: String,
}

/// The skill library changed: one was written, imported, deleted, switched or pinned (12 §A).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct SkillsChanged;

/// A memory was written, edited, deleted or restored (12 §B).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct MemoryChanged;
