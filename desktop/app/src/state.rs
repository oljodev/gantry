use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
    time::Instant,
};

use gantry_agent::{Artifacts, TurnManager};
use gantry_connectors::ConnectorRegistry;
use gantry_core::Settings;
use gantry_providers::ProviderRegistry;
use gantry_secrets::SecretVault;
use gantry_store::{BlobStore, Store};

/// Process-wide state managed by Tauri (`docs/plan/01-architecture-overview.md` §2).
pub struct AppState {
    /// The application data directory: database, blobs, skills.
    pub data_dir: PathBuf,
    /// Where the log plugin writes.
    pub log_dir: PathBuf,
    /// When this process started.
    pub started_at: Instant,
    pub store: Arc<Store>,
    /// Content-addressed files: attachments, artifact versions, command output.
    pub blobs: Arc<BlobStore>,
    pub secrets: Arc<SecretVault>,
    pub providers: Arc<ProviderRegistry>,
    /// The cached `settings` table; every write goes through `update_settings`.
    pub settings: Arc<RwLock<Settings>>,
    pub turns: Arc<TurnManager>,
    /// Every connector the turn loop can call, runtime tools included.
    pub tools: Arc<ConnectorRegistry>,
    /// Installing, connecting and authorizing (03 §7, §11).
    pub connectors: Arc<crate::connectors::ConnectorService>,
    pub artifacts: Arc<Artifacts>,
    /// Roots, file IO and the edit journal: what the Changes pane reads and reverts through.
    pub workspace: Arc<gantry_workspace::Workspace>,
    /// Providers whose last key test failed with an auth error; cleared when the key changes.
    pub invalid_keys: Mutex<HashSet<String>>,
}
