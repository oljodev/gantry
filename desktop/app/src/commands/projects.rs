//! Projects (docs/plan/09 M11): the page, its knowledge, its defaults, and moving a chat in or
//! out of one.
//!
//! Anything that changes what a project contributes to a prompt goes on to tell the chats in it
//! (10 §4): the ones that have not spoken are rebuilt, the ones that have are told, and the note
//! saying *what* changed is written here because this is the only layer that knows.

use gantry_core::{
    AttachmentInput, ChatId, ChatSummary, ErrorDto, GantryError, NewProject, ProjectDetail,
    ProjectFileDto, ProjectFileId, ProjectId, ProjectPatch, ProjectSummary,
};
use tauri::{AppHandle, State};
use tauri_specta::Event;

use crate::{
    AppState,
    events::{ChatsChanged, ProjectsChanged, SkillsChanged},
};

#[tauri::command]
#[specta::specta]
pub fn list_projects(state: State<'_, AppState>) -> Result<Vec<ProjectSummary>, ErrorDto> {
    Ok(state.projects.list()?)
}

#[tauri::command]
#[specta::specta]
pub fn get_project(
    state: State<'_, AppState>,
    project_id: ProjectId,
) -> Result<ProjectDetail, ErrorDto> {
    state
        .projects
        .get(project_id)?
        .ok_or_else(|| GantryError::not_found(format!("project {project_id}")).into())
}

#[tauri::command]
#[specta::specta]
pub fn create_project(
    app: AppHandle,
    state: State<'_, AppState>,
    project: NewProject,
) -> Result<ProjectSummary, ErrorDto> {
    let summary = state.projects.create(project)?;
    let _ = ProjectsChanged.emit(&app);
    Ok(summary)
}

/// Applies a patch and returns the whole project. A change to the instructions reaches every
/// chat in it the way a change to the global ones does (10 §4).
#[tauri::command]
#[specta::specta]
pub fn update_project(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: ProjectId,
    patch: ProjectPatch,
) -> Result<ProjectDetail, ErrorDto> {
    let instructions = patch.instructions.clone();
    let project = state.projects.update(project_id, patch)?;
    if instructions.is_some() {
        let text = project.instructions.trim();
        let note = if text.is_empty() {
            format!(
                "The user removed the instructions of the project \"{}\"; the earlier \
                 <instructions scope=\"project\"> no longer apply.",
                project.name
            )
        } else {
            format!(
                "Updated instructions for the project \"{}\" (replacing any earlier ones):\n\
                 <instructions scope=\"project\">\n{text}\n</instructions>",
                project.name
            )
        };
        state.turns.project_changed(project_id, note)?;
    }
    let _ = ProjectsChanged.emit(&app);
    Ok(project)
}

/// Deletes the project. Its chats come out of it rather than down with it (migration 0014), and
/// they are told, because their instructions and knowledge have just gone.
#[tauri::command]
#[specta::specta]
pub fn delete_project(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: ProjectId,
) -> Result<(), ErrorDto> {
    let name = state.projects.get(project_id)?.map(|p| p.name);
    let chats = state.projects.chat_ids(project_id)?;
    state.projects.delete(project_id)?;
    for chat in &chats {
        let note = format!(
            "The project \"{}\" was deleted. Its instructions and its knowledge files no longer \
             apply; this chat is unchanged otherwise.",
            name.as_deref().unwrap_or("this chat was in")
        );
        state.turns.chat_context_changed(*chat, note)?;
    }
    let _ = ProjectsChanged.emit(&app);
    let _ = ChatsChanged { chat_ids: chats }.emit(&app);
    Ok(())
}

/// Adds a knowledge file, text extracted (06 §8). The chats in the project are told what
/// arrived, and carry its text — a file added to answer a question in an open chat is no use to
/// that chat if only the next one can read it.
#[tauri::command]
#[specta::specta]
pub async fn add_project_file(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: ProjectId,
    file: AttachmentInput,
) -> Result<ProjectFileDto, ErrorDto> {
    // Reading and extracting is file IO and, for a document, seconds of parsing: off the thread
    // the window is drawn on, like `send_message`.
    let projects = state.projects.clone();
    let added = tokio::task::spawn_blocking(move || projects.add_file(project_id, file))
        .await
        .map_err(|_| GantryError::internal("adding the file was interrupted"))??;
    let project = state.projects.get(project_id)?;
    if let Some(project) = project {
        let text = state.projects.file_text(added.id)?.unwrap_or_default();
        let note = format!(
            "The user added \"{}\" to the knowledge of the project \"{}\". New chats here carry \
             its text; this is it:\n<file name=\"{}\">\n{}\n</file>",
            added.name,
            project.name,
            added.name,
            gantry_agent::projects::note_excerpt(&text)
        );
        state.turns.project_changed(project_id, note)?;
    }
    let _ = ProjectsChanged.emit(&app);
    Ok(added)
}

#[tauri::command]
#[specta::specta]
pub fn remove_project_file(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: ProjectId,
    file_id: ProjectFileId,
) -> Result<(), ErrorDto> {
    let name = state
        .projects
        .get(project_id)?
        .and_then(|p| p.files.into_iter().find(|f| f.id == file_id))
        .map(|f| f.name);
    state.projects.remove_file(project_id, file_id)?;
    if let Some(name) = name {
        state.turns.project_changed(
            project_id,
            format!(
                "The user removed \"{name}\" from this project's knowledge. Do not rely on what \
                 it said."
            ),
        )?;
    }
    let _ = ProjectsChanged.emit(&app);
    Ok(())
}

/// Pins a skill to the project, which pins it for every chat in it (12 §A6).
#[tauri::command]
#[specta::specta]
pub fn pin_skill_to_project(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: ProjectId,
    skill_id: String,
    pinned: bool,
) -> Result<(), ErrorDto> {
    state.projects.pin_skill(project_id, &skill_id, pinned)?;
    let note = if pinned {
        format!(
            "The skill \"{skill_id}\" is now pinned to this project: it is in the prompt of \
             every new chat here."
        )
    } else {
        format!(
            "The skill \"{skill_id}\" is no longer pinned to this project; it can still be \
             matched to a message like any other."
        )
    };
    state.turns.project_changed(project_id, note)?;
    let _ = SkillsChanged.emit(&app);
    let _ = ProjectsChanged.emit(&app);
    Ok(())
}

/// The chats filed in this project, newest activity first.
#[tauri::command]
#[specta::specta]
pub fn list_project_chats(
    state: State<'_, AppState>,
    project_id: ProjectId,
) -> Result<Vec<ChatSummary>, ErrorDto> {
    let ids = state.projects.chat_ids(project_id)?;
    let chats = state.turns.chats();
    Ok(ids
        .into_iter()
        .filter_map(|id| chats.summary(id).ok().flatten())
        .collect())
}

/// **Add to project** and **Move to project** (09 M11), and the same command for taking a chat
/// out of one.
#[tauri::command]
#[specta::specta]
pub fn set_chat_project(
    app: AppHandle,
    state: State<'_, AppState>,
    chat_id: ChatId,
    project_id: Option<ProjectId>,
) -> Result<(), ErrorDto> {
    state.turns.set_chat_project(chat_id, project_id)?;
    let _ = ProjectsChanged.emit(&app);
    let _ = ChatsChanged {
        chat_ids: vec![chat_id],
    }
    .emit(&app);
    Ok(())
}

/// **Continue in new chat** on an artifact (13 §9): a chat in the same project, told which
/// artifact it is about and to read it before working.
#[tauri::command]
#[specta::specta]
pub fn continue_artifact_in_new_chat(
    app: AppHandle,
    state: State<'_, AppState>,
    artifact_id: gantry_core::ArtifactId,
) -> Result<ChatSummary, ErrorDto> {
    let artifact = state
        .artifacts
        .get(artifact_id)
        .map_err(GantryError::from)?;
    let chat = state.turns.create_session(
        gantry_core::Surface::Chat,
        Vec::new(),
        None,
        false,
        artifact.project_id,
    )?;
    state.turns.chats().append_system_note(
        chat.id,
        format!(
            "This chat continues from the artifact \"{}\" ({}), made in another chat of this \
             project. Read it with gantry__read_artifact (id {}) before working on it, and do \
             not guess at what it contains.",
            artifact.title, artifact.artifact_type, artifact.id
        ),
    )?;
    let _ = ChatsChanged {
        chat_ids: vec![chat.id],
    }
    .emit(&app);
    Ok(chat)
}
