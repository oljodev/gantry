//! Startup: directories, logging, the store, the vault, settings, providers, the turn manager,
//! window chrome, state.

use std::{
    error::Error,
    fs,
    sync::{Arc, Mutex, RwLock},
    time::Instant,
};

use gantry_agent::{Artifacts, ChatBook, ChatNotifier, PromptContext, RuntimeTools, TurnManager};
use gantry_connectors::ConnectorRegistry;
use gantry_core::{ChatId, ProviderId, Settings};
use gantry_providers::{ProviderRegistry, http_client};
use gantry_secrets::SecretVault;
use gantry_store::{BlobStore, Store, repos};
use tauri::{App, AppHandle, Manager, plugin::TauriPlugin};
use tauri_plugin_log::{Target, TargetKind};
use tauri_specta::Event;

use crate::{
    AppState,
    events::{ChatsChanged, InteractionsChanged},
};

/// Turns that end and titles that arrive reach the frontend as `chats:changed`; pending
/// decisions as `interactions:changed`.
struct Notifier(AppHandle);

impl ChatNotifier for Notifier {
    fn chats_changed(&self, chat_ids: Vec<ChatId>) {
        let _ = ChatsChanged { chat_ids }.emit(&self.0);
    }

    fn interactions_changed(&self, chat_id: ChatId, pending: u32) {
        let _ = InteractionsChanged { chat_id, pending }.emit(&self.0);
    }
}

/// Logging to stdout, to `<app log dir>/gantry.log`, and to the webview console. Third-party
/// crates stay at `info`, so no HTTP library ever logs a request header.
pub fn log_plugin() -> TauriPlugin<tauri::Wry> {
    let level = if cfg!(debug_assertions) {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    };
    let mut builder = tauri_plugin_log::Builder::new()
        .targets([
            Target::new(TargetKind::Stdout),
            Target::new(TargetKind::LogDir {
                file_name: Some("gantry".into()),
            }),
            Target::new(TargetKind::Webview),
        ])
        .level(log::LevelFilter::Info);
    for target in [
        "gantry_app_lib",
        "gantry_core",
        "gantry_agent",
        "gantry_connectors",
        "gantry_providers",
        "gantry_secrets",
        "gantry_store",
    ] {
        builder = builder.level_for(target, level);
    }
    builder.build()
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

    let store = Arc::new(Store::open(data_dir.join("gantry.db"))?);
    let blobs = Arc::new(BlobStore::open(data_dir.join("blobs"))?);
    // Crash recovery (01 §3, 09 M2): whatever the previous process left running is closed and
    // every transcript is made replayable again.
    let recovered =
        store.write_blocking(|conn| repos::recovery::run(conn, gantry_core::now_ms()))?;
    if !recovered.is_empty() {
        log::warn!(
            "recovered from the previous shutdown: {} turn(s) interrupted, {} tool call(s) cancelled, {} prompt(s) cancelled, {} synthetic result(s) written",
            recovered.turns,
            recovered.tool_calls,
            recovered.interactions,
            recovered.results
        );
    }

    // The OS credential store is touched from a plain thread: its clients bring their own
    // event loops and must not be driven from inside an async runtime.
    let secrets = {
        let store = store.clone();
        let dir = data_dir.clone();
        std::thread::spawn(move || SecretVault::open(&dir, store))
            .join()
            .map_err(|_| "the secret store thread panicked")??
    };
    log::info!("secret store: {:?}", secrets.status());
    let secrets = Arc::new(secrets);

    let settings = Arc::new(RwLock::new(load_settings(&store)?));

    // The five accounts of 02 §1, one row each; custom endpoints are added from Settings.
    store.write_blocking(|conn| {
        repos::providers::ensure(
            conn,
            ProviderId::OPENROUTER,
            "openai_chat",
            "OpenRouter",
            Some("https://openrouter.ai/api/v1"),
        )?;
        repos::providers::ensure(conn, "anthropic", "anthropic", "Anthropic", None)?;
        repos::providers::ensure(conn, "openai", "openai_responses", "OpenAI", None)?;
        repos::providers::ensure(conn, "google", "gemini", "Google", None)?;
        repos::providers::ensure(
            conn,
            "xai",
            "openai_chat",
            "xAI",
            Some("https://api.x.ai/v1"),
        )
    })?;
    let providers = Arc::new(ProviderRegistry::new(
        store.clone(),
        secrets.clone(),
        http_client(env!("CARGO_PKG_VERSION")),
    ));
    providers.rebuild()?;

    // Runtime tools are always registered; installed connectors join the registry with M9.
    let artifacts = Arc::new(Artifacts::new(store.clone(), blobs.clone()));
    let connectors = Arc::new(ConnectorRegistry::new());
    connectors.register(Arc::new(RuntimeTools::with_artifacts(artifacts.clone())));

    let turns = TurnManager::new(
        Arc::new(ChatBook::new(store.clone(), blobs)),
        providers.clone(),
        connectors,
        settings.clone(),
        PromptContext {
            platform: std::env::consts::OS.to_owned(),
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
            workspace_roots: Vec::new(),
            project_name: None,
        },
        tauri::async_runtime::handle().inner().clone(),
    );
    turns.set_notifier(Arc::new(Notifier(app.handle().clone())));

    app.manage(AppState {
        data_dir,
        log_dir,
        started_at: Instant::now(),
        store,
        secrets,
        providers,
        settings,
        turns,
        artifacts,
        invalid_keys: Mutex::new(Default::default()),
    });
    Ok(())
}

/// The settings document from its section rows; a missing or unreadable section keeps its
/// default (11 §1).
fn load_settings(store: &Store) -> Result<Settings, gantry_store::StoreError> {
    let rows = store.read(repos::settings::all)?;
    let mut doc = serde_json::Map::new();
    for (key, json) in rows {
        match serde_json::from_str::<serde_json::Value>(&json) {
            Ok(v) => {
                doc.insert(key, v);
            }
            Err(err) => log::warn!("settings section {key} is unreadable ({err}); using defaults"),
        }
    }
    Ok(serde_json::from_value(serde_json::Value::Object(doc)).unwrap_or_default())
}
