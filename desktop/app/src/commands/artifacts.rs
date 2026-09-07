//! Artifacts (docs/plan/13 §10): the panel's reads, the user's edits and restores as versions
//! with the transcript note of 13 §7, export through the save dialog, the render report that
//! completes a tool result, and the separate window.

use gantry_core::{
    ArtifactContent, ArtifactDto, ArtifactId, ChatId, ErrorDto, GantryError, RenderReport,
    VersionSource,
};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_specta::Event;

use crate::{AppState, events::ArtifactsChanged};

/// Artifacts of one chat (oldest first), of one project, or of every chat (the library,
/// newest change first).
#[tauri::command]
#[specta::specta]
pub fn list_artifacts(
    state: State<'_, AppState>,
    chat_id: Option<ChatId>,
    project_id: Option<String>,
) -> Result<Vec<ArtifactDto>, ErrorDto> {
    let list = match (chat_id, project_id) {
        (Some(c), _) => state.artifacts.list_for_chat(c),
        (None, Some(p)) => state.artifacts.list_for_project(&p),
        (None, None) => state.artifacts.list_all(),
    };
    Ok(list.map_err(GantryError::from)?)
}

/// The current version with its history.
#[tauri::command]
#[specta::specta]
pub fn get_artifact(
    state: State<'_, AppState>,
    artifact_id: ArtifactId,
) -> Result<ArtifactContent, ErrorDto> {
    Ok(state
        .artifacts
        .read(artifact_id, None)
        .map_err(GantryError::from)?)
}

#[tauri::command]
#[specta::specta]
pub fn get_artifact_version(
    state: State<'_, AppState>,
    artifact_id: ArtifactId,
    version: u32,
) -> Result<ArtifactContent, ErrorDto> {
    Ok(state
        .artifacts
        .read(artifact_id, Some(version))
        .map_err(GantryError::from)?)
}

fn note_user_change(
    state: &AppState,
    artifact: &ArtifactDto,
    version: u32,
    content: &str,
    source: VersionSource,
) {
    let note = gantry_agent::user_change_note(artifact, version, content, source);
    if let Err(err) = state
        .turns
        .chats()
        .append_system_note(artifact.chat_id, note)
    {
        log::warn!("could not note the user's artifact change: {err}");
    }
}

/// The user saved the source in the panel: a `user_edit` version plus the transcript note.
#[tauri::command]
#[specta::specta]
pub fn save_artifact_version(
    app: AppHandle,
    state: State<'_, AppState>,
    artifact_id: ArtifactId,
    content: String,
) -> Result<ArtifactContent, ErrorDto> {
    let (artifact, version) = state
        .artifacts
        .save_user_version(artifact_id, content.clone())
        .map_err(GantryError::from)?;
    note_user_change(
        &state,
        &artifact,
        version,
        &content,
        VersionSource::UserEdit,
    );
    let _ = ArtifactsChanged {
        chat_id: artifact.chat_id,
        artifact_id,
    }
    .emit(&app);
    Ok(state
        .artifacts
        .read(artifact_id, None)
        .map_err(GantryError::from)?)
}

/// A new version with an earlier version's content (13 §7).
#[tauri::command]
#[specta::specta]
pub fn restore_artifact_version(
    app: AppHandle,
    state: State<'_, AppState>,
    artifact_id: ArtifactId,
    version: u32,
) -> Result<ArtifactContent, ErrorDto> {
    let (artifact, new_version) = state
        .artifacts
        .restore(artifact_id, version)
        .map_err(GantryError::from)?;
    let current = state
        .artifacts
        .read(artifact_id, None)
        .map_err(GantryError::from)?;
    note_user_change(
        &state,
        &artifact,
        new_version,
        &current.content,
        VersionSource::UserRestore,
    );
    let _ = ArtifactsChanged {
        chat_id: artifact.chat_id,
        artifact_id,
    }
    .emit(&app);
    Ok(current)
}

/// Writes one version to a file the user picks; `None` when the dialog was dismissed.
#[tauri::command]
#[specta::specta]
pub fn export_artifact(
    app: AppHandle,
    state: State<'_, AppState>,
    artifact_id: ArtifactId,
    version: Option<u32>,
) -> Result<Option<String>, ErrorDto> {
    let content = state
        .artifacts
        .read(artifact_id, version)
        .map_err(GantryError::from)?;
    let ext = gantry_agent::artifacts::registry::extension_for(
        &content.artifact.artifact_type,
        content.artifact.language.as_deref(),
    );
    let stem: String = content
        .artifact
        .title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim()
        .replace(' ', "-");
    let name = format!(
        "{}.{ext}",
        if stem.is_empty() {
            "artifact".to_owned()
        } else {
            stem
        }
    );
    let Some(path) = app
        .dialog()
        .file()
        .set_file_name(&name)
        .add_filter(content.artifact.artifact_type.clone(), &[ext.as_str()])
        .blocking_save_file()
    else {
        return Ok(None);
    };
    let path = path
        .into_path()
        .map_err(|e| GantryError::invalid(format!("not a file path: {e}")))?;
    std::fs::write(&path, content.content.as_bytes())
        .map_err(|e| GantryError::invalid(format!("could not write {}: {e}", path.display())))?;
    Ok(Some(path.display().to_string()))
}

/// The panel's report for one version: completes a tool result waiting on it (13 §2).
#[tauri::command]
#[specta::specta]
pub fn report_artifact_render(
    state: State<'_, AppState>,
    artifact_id: ArtifactId,
    version: u32,
    report: RenderReport,
) -> Result<bool, ErrorDto> {
    Ok(state.artifacts.report_render(artifact_id, version, report))
}

/// Opens the artifact in its own window (13 §4, §5): a separate webview, which on every
/// platform is at least a separate document and on most a separate process. The frontend
/// router uses hash history, so the route sits behind `index.html#`; the window gets the same
/// frameless treatment as the main one (the app draws its own title strip, `startup.rs`).
#[tauri::command]
#[specta::specta]
pub fn open_artifact_window(
    app: AppHandle,
    state: State<'_, AppState>,
    artifact_id: ArtifactId,
) -> Result<(), ErrorDto> {
    let label = format!("artifact-{artifact_id}");
    if let Some(existing) = app.get_webview_window(&label) {
        let _ = existing.set_focus();
        return Ok(());
    }
    let title = state
        .artifacts
        .read(artifact_id, None)
        .map(|c| c.artifact.title)
        .unwrap_or_else(|_| "Artifact".to_string());
    let url =
        tauri::WebviewUrl::App(format!("index.html#/artifact-window?id={artifact_id}").into());
    let builder = tauri::WebviewWindowBuilder::new(&app, &label, url)
        .title(format!("{title} · Gantry"))
        .inner_size(900.0, 700.0)
        .min_inner_size(400.0, 300.0);
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    #[cfg(not(target_os = "macos"))]
    let builder = builder.decorations(false);
    builder
        .build()
        .map_err(|e| GantryError::invalid(format!("could not open the window: {e}")))?;
    Ok(())
}
