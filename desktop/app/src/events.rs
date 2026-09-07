//! Global invalidation events (docs/plan/01 §4): ids only, never data.

use gantry_core::ChatId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct ChatsChanged {
    pub chat_ids: Vec<ChatId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct ProvidersChanged;

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct SettingsChanged;
