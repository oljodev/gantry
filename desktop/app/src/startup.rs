//! Startup: directories, logging, the store, the vault, settings, providers, the turn manager,
//! window chrome, state.

use std::{
    error::Error,
    fs,
    sync::{Arc, Mutex, RwLock},
    time::Instant,
};

use gantry_agent::{
    Artifacts, ChatBook, ChatNotifier, ConnectorAccess, Memories, PromptContext, RuntimeTools,
    Skills, TurnManager,
    runtime_tools::{memory::MemoryTools, skills::SkillTools},
};
use gantry_connectors::ConnectorRegistry;
use gantry_core::{ChatId, ProviderId, Settings};
use gantry_providers::{ProviderRegistry, http_client};
use gantry_secrets::SecretVault;
use gantry_store::{BlobStore, Store, repos};
use gantry_workspace::Workspace;
use tauri::{App, AppHandle, Manager, plugin::TauriPlugin};
use tauri_plugin_log::{Target, TargetKind};
use tauri_specta::Event;

use crate::{
    AppState,
    connectors::ConnectorService,
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

    // The window's own background, before the webview has painted anything (15 A20).
    //
    // `backgroundColor` in the config is one colour for every machine, and it was the dark one:
    // the first frame of the first launch on a light desktop was a dark rectangle that flipped
    // white a moment later. The setting decides it where the user has chosen, and the OS does
    // where they have left it on System — which is the same rule the webview follows a few
    // milliseconds later, so the two now agree.
    if let Some(window) = app.get_webview_window("main") {
        let dark = match settings
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .appearance
            .theme
        {
            gantry_core::Theme::Dark => true,
            gantry_core::Theme::Light => false,
            gantry_core::Theme::System => window.theme().is_ok_and(|t| t == tauri::Theme::Dark),
        };
        let _ = window.set_background_color(Some(if dark {
            tauri::window::Color(0x11, 0x11, 0x13, 0xff)
        } else {
            tauri::window::Color(0xf4, 0xf4, 0xf5, 0xff)
        }));
    }

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

    // Runtime tools are always registered; installed connectors join them below.
    let artifacts = Arc::new(Artifacts::new(store.clone(), blobs.clone()));
    let tools = Arc::new(ConnectorRegistry::new());
    // Roots, atomic file IO and the edit journal, shared by every native connector (03 §5).
    let workspace = Arc::new(Workspace::new(
        store.clone(),
        blobs.clone(),
        data_dir.clone(),
    ));
    // The login shell answers once, at startup: a GUI app otherwise runs commands with a
    // nearly empty PATH on macOS (`docs/connectors/shell.md` D2).
    let shell_env = Arc::new(gantry_connector_shell::ShellEnv::capture());
    let connectors = Arc::new(ConnectorService::new(
        store.clone(),
        secrets.clone(),
        tools.clone(),
        crate::native::Deps {
            workspace: workspace.clone(),
            shell_env,
            data_dir: data_dir.clone(),
            providers: providers.clone(),
            store: store.clone(),
            settings: settings.clone(),
        },
    ));

    let turns = TurnManager::new(
        Arc::new(ChatBook::new(store.clone(), blobs.clone())),
        providers.clone(),
        tools.clone(),
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

    // Skills and memory (12). The skill library is handed to the turn manager, which rescans
    // the folder before each turn; memory needs no such hand-off, because the selector reads
    // the same tables from inside the turn's own transaction.
    let skills = Skills::new(store.clone(), data_dir.join("skills"));
    if let Err(err) = skills.rescan() {
        log::warn!("could not index the skills folder: {err}");
    }
    turns.set_skills(skills.clone());
    // An incognito session lives as long as its window (15 A21). Nothing but a crash can leave
    // one behind, and this is where that one case is answered — before any list can read it.
    match turns.chats().sweep_incognito() {
        Ok(0) => {}
        Ok(n) => log::info!("deleted {n} incognito session(s) left by the previous run"),
        Err(err) => log::warn!("could not delete the incognito sessions left behind: {err}"),
    }
    let projects = Arc::new(gantry_agent::Projects::new(store.clone(), blobs.clone()));
    let memories = Memories::new(store.clone());
    // A memory the user changed reaches the chats it was frozen into (12 §B6): every write
    // goes through `Memories`, so the rule is installed here once rather than at each of the
    // five callers — the page's four commands and the model's own tools. Weak, because the
    // turn manager reaches back here through the tool set and a cycle would outlive the app.
    memories.set_on_change({
        let turns = Arc::downgrade(&turns);
        Arc::new(move |entry: &gantry_core::MemoryDto, edit, except| {
            let Some(turns) = turns.upgrade() else { return };
            if let Err(err) = turns.memory_changed(entry, edit, except) {
                log::warn!("could not tell the open chats about a memory change: {err}");
            }
        })
    });
    // The weekly blob sweep (06 §3, §8). Detached rather than awaited: it walks the whole blob
    // directory and reads every message's parts, which on a large history is not something to
    // hold the window open for, and nothing that follows depends on it.
    {
        let blobs = blobs.clone();
        store.write_detached(move |conn| {
            match gantry_store::sweep::if_due(conn, &blobs)? {
                None => {}
                Some(report) if report.is_empty() => log::info!("swept blobs: nothing to remove"),
                Some(report) => log::info!(
                    "swept blobs: removed {} file(s), {} bytes",
                    report.files,
                    report.bytes
                ),
            }
            Ok(())
        });
    }
    // Recently deleted is thirty days, and this is the only place that notices they are up.
    match memories.sweep() {
        Ok(0) => {}
        Ok(n) => log::info!("swept {n} memories out of Recently deleted"),
        Err(err) => log::warn!("could not sweep Recently deleted: {err}"),
    }
    // The connector tools ask the user through the same interaction registry the permission
    // cards use (03 §9, 04 §9), so they are registered once the turn manager owns it.
    tools.register(Arc::new(
        RuntimeTools::with_artifacts(artifacts.clone())
            .with_connectors(ConnectorAccess::new(
                store.clone(),
                tools.clone(),
                turns.interactions().clone(),
                settings.clone(),
            ))
            .with_library(
                SkillTools::new(skills.clone(), turns.interactions().clone()),
                MemoryTools::new(
                    memories.clone(),
                    turns.interactions().clone(),
                    settings.clone(),
                ),
            ),
    ));

    app.manage(AppState {
        data_dir,
        log_dir,
        started_at: Instant::now(),
        store,
        blobs,
        secrets,
        providers,
        settings,
        turns,
        tools,
        connectors: connectors.clone(),
        artifacts,
        skills,
        memories,
        projects,
        workspace,
        invalid_keys: Mutex::new(Default::default()),
    });

    // Installed connectors are registered without waiting for the window: a chat that starts
    // immediately still sees its tools. A server that cannot be reached is logged, not fatal.
    tauri::async_runtime::spawn(async move {
        // The app's own connectors first, so the first run has them without anybody installing
        // anything (03 §11). It is a no-op on every run after the first.
        connectors.install_defaults().await;
        if let Err(err) = connectors.rebuild().await {
            log::warn!("registering the installed connectors: {err}");
        }
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
