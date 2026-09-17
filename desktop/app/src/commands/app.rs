use gantry_core::{AppInfo, ErrorDto, StartupTiming};
use tauri::State;

use crate::AppState;

/// Facts about the running app: version, OS, directories. The M0 round-trip command.
#[tauri::command]
#[specta::specta]
pub fn app_info(state: State<'_, AppState>) -> Result<AppInfo, ErrorDto> {
    Ok(AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        data_dir: state.data_dir.display().to_string(),
        log_dir: state.log_dir.display().to_string(),
        debug: cfg!(debug_assertions),
    })
}

/// What startup spent, and how long ago it was (`perf.rs`). Read by Settings → Advanced, which
/// joins it to the webview's own marks.
#[tauri::command]
#[specta::specta]
pub fn startup_timing() -> StartupTiming {
    crate::perf::timing()
}
