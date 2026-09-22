use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
    time::Instant,
};

use gantry_agent::{Artifacts, Projects, TurnManager};
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
    /// The skill library (12 §A): the index, the folder, and the four flows.
    pub skills: Arc<gantry_agent::Skills>,
    /// What Gantry remembers (12 §B).
    pub memories: Arc<gantry_agent::Memories>,
    /// Projects: their instructions, knowledge, defaults and the chats filed in them (09 M11).
    pub projects: Arc<Projects>,
    /// Roots, file IO and the edit journal: what the Changes pane reads and reverts through.
    pub workspace: Arc<gantry_workspace::Workspace>,
    /// The user's own terminals (16 §5). Not a tool, and not reachable by a model: a pty per
    /// open tab, running the user's login shell.
    pub terminals: Arc<gantry_terminal::Terminals>,
    /// Providers whose last key test failed with an auth error; cleared when the key changes.
    pub invalid_keys: Mutex<HashSet<String>>,
}
