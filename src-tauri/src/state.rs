use std::{path::PathBuf, time::Instant};

/// Process-wide state managed by Tauri. Grows with the milestones
/// (`docs/plan/01-architecture-overview.md` §2, "AppState").
pub struct AppState {
    /// The application data directory: database, blobs, skills.
    pub data_dir: PathBuf,
    /// Where the log plugin writes.
    pub log_dir: PathBuf,
    /// When this process started.
    pub started_at: Instant,
}
