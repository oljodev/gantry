//! Startup: directories, logging, window chrome, state.

use std::{error::Error, fs, time::Instant};

use tauri::{App, Manager, plugin::TauriPlugin};
use tauri_plugin_log::{Target, TargetKind};

use crate::AppState;

/// Logging to stdout, to `<app log dir>/gantry.log`, and to the webview console.
pub fn log_plugin() -> TauriPlugin<tauri::Wry> {
    let level = if cfg!(debug_assertions) {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    };
    tauri_plugin_log::Builder::new()
        .targets([
            Target::new(TargetKind::Stdout),
            Target::new(TargetKind::LogDir {
                file_name: Some("gantry".into()),
            }),
            Target::new(TargetKind::Webview),
        ])
        .level(log::LevelFilter::Info)
        .level_for("gantry_app_lib", level)
        .level_for("gantry_core", level)
        .build()
}

pub fn init(app: &mut App) -> Result<(), Box<dyn Error>> {
    let data_dir = app.path().app_data_dir()?;
    let log_dir = app.path().app_log_dir()?;
    for dir in [&data_dir, &log_dir] {
        fs::create_dir_all(dir)?;
    }
    for sub in ["blobs", "skills"] {
        fs::create_dir_all(data_dir.join(sub))?;
    }
    log::info!(
        "Gantry {} starting; data dir {}; log dir {}",
        env!("CARGO_PKG_VERSION"),
        data_dir.display(),
        log_dir.display()
    );

    // The title strip is drawn by the app on every OS. macOS keeps its decorations in Overlay
    // mode (the traffic lights); Windows and Linux drop the native frame here rather than in
    // the config, because per-platform config files replace the whole `windows` array.
    #[cfg(not(target_os = "macos"))]
    if let Some(window) = app.get_webview_window("main") {
        window.set_decorations(false)?;
    }

    app.manage(AppState {
        data_dir,
        log_dir,
        started_at: Instant::now(),
    });
    Ok(())
}
