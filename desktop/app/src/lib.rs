//! The Gantry desktop application: Tauri builder, plugins, state and commands.
//!
//! Everything that knows about Tauri lives in this crate; the rest of the workspace is plain
//! Rust. See `docs/plan/01-architecture-overview.md` §2 and §4.

#![forbid(unsafe_code)]

mod commands;
mod connectors;
mod events;
mod native;
mod startup;
mod state;

pub use state::AppState;

/// Every command and event, collected once so the invoke handler and the TypeScript export
/// cannot drift apart.
fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new()
        .commands(tauri_specta::collect_commands![
            commands::app::app_info,
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::settings::get_guardrails,
            commands::settings::list_guard_decisions,
            commands::settings::get_secret_store_status,
            commands::settings::get_data_info,
            commands::settings::open_data_dir,
            commands::settings::backup_database,
            commands::settings::maintain_database,
            commands::providers::list_providers,
            commands::providers::set_provider_key,
            commands::providers::clear_provider_key,
            commands::providers::test_provider,
            commands::providers::list_models,
            commands::providers::update_provider,
            commands::providers::add_custom_provider,
            commands::providers::remove_provider,
            commands::chats::create_chat,
            commands::chats::image_preview,
            commands::chats::blob_image,
            commands::chats::blob_media,
            commands::chats::list_chats,
            commands::code::session_changes,
            commands::code::session_file_diff,
            commands::code::revert_file,
            commands::code::revert_session,
            commands::chats::add_chat_root,
            commands::chats::remove_chat_root,
            commands::chats::get_chat,
            commands::chats::update_chat,
            commands::chats::delete_chat,
            commands::chats::rate_turn,
            commands::chats::search,
            commands::chats::get_system_prompt,
            commands::chats::export_chat,
            commands::turns::send_message,
            commands::turns::retry_turn,
            commands::turns::allow_blocked_call,
            commands::turns::mark_judge_decision,
            commands::turns::cancel_turn,
            commands::turns::subscribe_turn,
            commands::turns::list_active_turns,
            commands::interactions::list_pending_interactions,
            commands::interactions::resolve_interaction,
            commands::interactions::list_chat_grants,
            commands::interactions::revoke_chat_grant,
            commands::interactions::revoke_all_chat_grants,
            commands::interactions::get_tool_call,
            commands::artifacts::list_artifacts,
            commands::artifacts::get_artifact,
            commands::artifacts::get_artifact_version,
            commands::artifacts::save_artifact_version,
            commands::artifacts::restore_artifact_version,
            commands::artifacts::export_artifact,
            commands::artifacts::report_artifact_render,
            commands::artifacts::open_artifact_window,
            commands::connectors::list_catalog,
            commands::connectors::list_connectors,
            commands::connectors::get_connector,
            commands::connectors::check_runtimes,
            commands::connectors::connector_logs,
            commands::connectors::get_connector_config,
            commands::connectors::set_connector_config,
            commands::connectors::install_connector,
            commands::connectors::install_custom_connector,
            commands::connectors::connect_connector,
            commands::connectors::authorize_connector,
            commands::connectors::set_connector_token,
            commands::connectors::set_connector_enabled,
            commands::connectors::remove_connector,
            commands::connectors::list_chat_connectors,
            commands::connectors::attach_connector,
            commands::skills::list_skills,
            commands::skills::get_skill,
            commands::skills::save_skill,
            commands::skills::delete_skill,
            commands::skills::set_skill_enabled,
            commands::skills::test_skill_match,
            commands::skills::export_skill,
            commands::skills::review_skill,
            commands::skills::review_skill_url,
            commands::skills::install_skill,
            commands::skills::list_chat_skills,
            commands::skills::pin_skill_to_chat,
            commands::memory::list_memories,
            commands::memory::create_memory,
            commands::memory::update_memory,
            commands::memory::delete_memory,
            commands::memory::restore_memory,
            commands::memory::forget_memory_for_good,
            commands::memory::export_memories,
            commands::memory::review_memory_import,
            commands::memory::import_memories,
        ])
        .events(tauri_specta::collect_events![
            events::ChatsChanged,
            events::ProvidersChanged,
            events::SettingsChanged,
            events::InteractionsChanged,
            events::ArtifactsChanged,
            events::ConnectorsChanged,
            events::DeviceCodeNeeded,
            events::SkillsChanged,
            events::MemoryChanged,
        ])
}

/// The TypeScript export configuration shared by the debug-build export and the bindings test.
fn typescript() -> specta_typescript::Typescript {
    // Sequence numbers, token counts and timestamps are u64/i64 in Rust; JavaScript numbers
    // hold them exactly up to 2^53, far beyond anything a desktop app reaches.
    specta_typescript::Typescript::default().header(
        "// Generated by tauri-specta from the Rust commands. Do not edit.\n// Regenerate with `cargo xtask gen-bindings`; CI fails on drift.\n",
    )
}

pub fn run() {
    let builder = specta_builder();

    #[cfg(debug_assertions)]
    builder
        .export(typescript(), "../frontend/src/bindings.ts")
        .expect("failed to export the TypeScript bindings");

    let mut app = tauri::Builder::default();

    // The single-instance plugin must be registered before every other plugin.
    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    {
        app = app
            .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
                use tauri::Manager;
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.set_focus();
                }
            }))
            .plugin(tauri_plugin_window_state::Builder::default().build());
    }

    app.plugin(startup::log_plugin())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_os::init())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            startup::init(app)
        })
        .run(tauri::generate_context!())
        .expect("error while running Gantry");
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    /// Writes the bindings to `GANTRY_BINDINGS_OUT` when set (`cargo xtask gen-bindings`);
    /// otherwise regenerates into a temp file and fails if the committed file differs, so a
    /// plain `cargo test` is also the drift check.
    #[test]
    fn gen_bindings() {
        let committed =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../frontend/src/bindings.ts");
        match std::env::var_os("GANTRY_BINDINGS_OUT") {
            Some(out) => super::specta_builder()
                .export(super::typescript(), out)
                .unwrap(),
            None => {
                let tmp =
                    std::env::temp_dir().join(format!("gantry-bindings-{}.ts", std::process::id()));
                super::specta_builder()
                    .export(super::typescript(), &tmp)
                    .unwrap();
                let fresh = std::fs::read_to_string(&tmp).unwrap();
                let _ = std::fs::remove_file(&tmp);
                let current = std::fs::read_to_string(&committed).unwrap_or_default();
                assert_eq!(
                    fresh, current,
                    "desktop/frontend/src/bindings.ts is out of date: run `cargo xtask gen-bindings`"
                );
            }
        }
    }
}
