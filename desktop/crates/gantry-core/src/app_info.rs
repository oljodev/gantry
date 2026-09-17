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

/// What the backend spent before the window could paint, returned by `startup_timing`.
///
/// One number per phase of `startup::init`, in the order they ran, plus the clock the
/// frontend joins its own marks to: `since_start_ms` is how long ago the process began, read
/// at the moment of the call, so a mark taken in the webview can be placed on the same line
/// as the work that happened before the webview existed (docs/dev/performance.md).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct StartupTiming {
    /// First line of `run()` to the end of `setup`.
    pub total_ms: u32,
    /// Each phase, in the order it ran.
    pub phases: Vec<StartupPhase>,
    /// First line of `run()` to this call.
    pub since_start_ms: u32,
}

/// One phase of startup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct StartupPhase {
    /// A few words, in the app's own vocabulary: "database", "keychain", "login shell".
    pub name: String,
    pub ms: u32,
}
