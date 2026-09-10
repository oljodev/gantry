//! What a code session changed, and putting it back (docs/plan/16 §5).
//!
//! The Changes pane is the running answer to "what has it actually done to my repository", so
//! these read the journal rather than the conversation: a file that four tool calls edited is
//! one row with one diff, and Revert replays it from the other side.

use gantry_core::{ChatId, EditOp, ErrorDto, GantryError};
use gantry_workspace::{Roots, WorkspaceError};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::AppState;

/// One file in the pane's list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct FileChangeDto {
    /// Absolute, and the handle every other command here takes.
    pub path: String,
    /// The same path as the session sees it: relative to the folder it is working in.
    pub display: String,
    pub op: EditOp,
    pub added: u32,
    pub removed: u32,
    pub edits: u32,
    #[specta(type = specta_typescript::Number)]
    pub last_at: i64,
    /// A file whose versions are not text: listed and revertible, with no diff to draw.
    pub binary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct HunkDto {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    /// The hunk as unified-diff text, prefixes included.
    pub text: String,
}

/// A file's whole-session diff, for the pane under the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct FileDiffDto {
    pub path: String,
    pub display: String,
    pub op: EditOp,
    pub added: u32,
    pub removed: u32,
    pub binary: bool,
    pub hunks: Vec<HunkDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct RevertedDto {
    pub path: String,
    pub display: String,
    pub op: EditOp,
    pub added: u32,
    pub removed: u32,
    pub edits: u32,
}

/// The result of **Revert all**. A file that cannot go back does not stop the others: each is
/// its own decision, and the ones that failed are named with the reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct RevertAllDto {
    pub reverted: Vec<RevertedDto>,
    pub failed: Vec<RevertFailureDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct RevertFailureDto {
    pub path: String,
    pub display: String,
    pub reason: String,
}

/// Every file this session changed and has not put back, most recently changed first.
#[tauri::command]
#[specta::specta]
pub fn session_changes(
    state: State<'_, AppState>,
    chat_id: ChatId,
) -> Result<Vec<FileChangeDto>, ErrorDto> {
    let roots = state.workspace.roots(chat_id).ok();
    Ok(state
        .workspace
        .changes(chat_id)
        .map_err(refuse)?
        .into_iter()
        .map(|c| FileChangeDto {
            display: relative(&c.path, roots.as_ref()),
            path: c.path,
            op: c.op,
            added: count(c.added),
            removed: count(c.removed),
            edits: count(c.edits),
            last_at: c.last_at,
            binary: c.binary,
        })
        .collect())
}

/// One file's diff, from what it was before this session to what it is now.
#[tauri::command]
#[specta::specta]
pub fn session_file_diff(
    state: State<'_, AppState>,
    chat_id: ChatId,
    path: String,
) -> Result<FileDiffDto, ErrorDto> {
    let roots = state.workspace.roots(chat_id).ok();
    let change = state
        .workspace
        .file_change(chat_id, &path)
        .map_err(refuse)?;
    Ok(FileDiffDto {
        display: relative(&change.path, roots.as_ref()),
        path: change.path,
        op: change.op,
        added: count(change.diff.added),
        removed: count(change.diff.removed),
        binary: change.binary,
        hunks: change
            .diff
            .hunks
            .into_iter()
            .map(|h| HunkDto {
                old_start: count(h.old_start),
                old_lines: count(h.old_lines),
                new_start: count(h.new_start),
                new_lines: count(h.new_lines),
                text: h.text,
            })
            .collect(),
    })
}

/// Puts one file back to what it was before this session touched it.
#[tauri::command]
#[specta::specta]
pub async fn revert_file(
    state: State<'_, AppState>,
    chat_id: ChatId,
    path: String,
) -> Result<RevertedDto, ErrorDto> {
    let roots = state.workspace.roots(chat_id).map_err(refuse)?;
    let reverted = state
        .workspace
        .revert_file(&roots, chat_id, &path)
        .await
        .map_err(refuse)?;
    Ok(dto(reverted, Some(&roots)))
}

/// Puts every file back. Each file is reverted on its own, and one that refuses — because
/// something else has written it since — is reported rather than taking the rest down with it.
#[tauri::command]
#[specta::specta]
pub async fn revert_session(
    state: State<'_, AppState>,
    chat_id: ChatId,
) -> Result<RevertAllDto, ErrorDto> {
    let roots = state.workspace.roots(chat_id).map_err(refuse)?;
    let mut out = RevertAllDto {
        reverted: Vec::new(),
        failed: Vec::new(),
    };
    // Newest first, which is the order `changes` gives: a file created and then edited comes
    // back to nothing in one step either way, but reverting in the order the pane shows makes
    // the list empty from the top down while it runs.
    for change in state.workspace.changes(chat_id).map_err(refuse)? {
        match state
            .workspace
            .revert_file(&roots, chat_id, &change.path)
            .await
        {
            Ok(reverted) => out.reverted.push(dto(reverted, Some(&roots))),
            Err(err) => out.failed.push(RevertFailureDto {
                display: relative(&change.path, Some(&roots)),
                path: change.path,
                reason: err.to_string(),
            }),
        }
    }
    Ok(out)
}

fn dto(reverted: gantry_workspace::Reverted, roots: Option<&Roots>) -> RevertedDto {
    RevertedDto {
        display: relative(&reverted.path, roots),
        path: reverted.path,
        op: reverted.op,
        added: count(reverted.added),
        removed: count(reverted.removed),
        edits: count(reverted.edits),
    }
}

/// The path as the person reading it thinks of it: `src/auth.rs`, not the whole absolute path
/// of a folder they chose themselves. A file under none of the roots keeps its full path,
/// because then the folder really is the useful part.
fn relative(path: &str, roots: Option<&Roots>) -> String {
    let Some(roots) = roots else {
        return path.to_owned();
    };
    for root in roots.paths() {
        if let Some(rest) = path.strip_prefix(&root)
            && let Some(rest) = rest.strip_prefix(std::path::MAIN_SEPARATOR)
            && !rest.is_empty()
        {
            return rest.to_owned();
        }
    }
    path.to_owned()
}

/// A workspace refusal as the command layer's error. Only the message reaches the interface,
/// and the workspace already writes it for a person: what happened, and what to do about it.
fn refuse(err: WorkspaceError) -> ErrorDto {
    GantryError::invalid(err.to_string()).into()
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}
