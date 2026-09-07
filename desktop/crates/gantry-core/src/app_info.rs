use serde::{Deserialize, Serialize};

/// Facts about the running application, returned by the `app_info` command.
///
/// The first typed value to cross the IPC boundary; the bindings pipeline is proven on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct AppInfo {
    /// Application version from `Cargo.toml`.
    pub version: String,
    /// Operating system family: `macos`, `windows` or `linux`.
    pub os: String,
    /// CPU architecture, e.g. `aarch64` or `x86_64`.
    pub arch: String,
    /// The application data directory (database, blobs, skills, logs).
    pub data_dir: String,
    /// The log directory.
    pub log_dir: String,
    /// Whether this is a debug build.
    pub debug: bool,
}
