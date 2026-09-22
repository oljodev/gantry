//! The terminal tab (docs/plan/16 §5).
//!
//! A pty per tab, keyed by the tab's own id, with the bytes going out as events and the
//! keystrokes coming back as commands. Nothing here is a tool: the model cannot open a
//! terminal, type into one, or read one, and the shell connector it does have is a different
//! mechanism for a different job (`docs/connectors/shell.md` D3).

use std::{path::PathBuf, sync::Arc};

use gantry_core::{ChatId, ErrorDto, GantryError};
use gantry_terminal::{Open, TerminalSink};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_specta::Event;

use crate::{
    AppState,
    events::{TerminalExited, TerminalOutput},
};

/// What a terminal is, for the tab that draws it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct TerminalDto {
    pub id: String,
    /// The shell that is running, named as a person would: `zsh`, `bash`, `powershell`.
    pub shell: String,
    /// Where it started. Empty when it started in the user's home folder.
    pub cwd: String,
    /// What the terminal has already printed, for a tab being drawn again after it was closed,
    /// the chat was switched, or the window was reloaded. Empty for a terminal just opened.
    pub scrollback: String,
}

/// Emits a terminal's output to the window it belongs to.
struct WindowSink(AppHandle);

impl TerminalSink for WindowSink {
    fn output(&self, id: &str, data: &str) {
        let _ = TerminalOutput {
            id: id.to_owned(),
            data: data.to_owned(),
        }
        .emit(&self.0);
    }

    fn exited(&self, id: &str, code: Option<i32>) {
        let _ = TerminalExited {
            id: id.to_owned(),
            code,
        }
        .emit(&self.0);
    }
}

/// Opens the terminal for a tab, or reattaches to the one already running under that id.
///
/// The chat is where it starts: a code session's folder, and the user's home folder when there
/// is none — a terminal with nowhere to be is still a terminal, and refusing to open one in a
/// chat that has no folder attached would be a puzzle rather than a safeguard.
#[tauri::command]
#[specta::specta]
pub fn open_terminal(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    chat_id: Option<ChatId>,
    cols: u16,
    rows: u16,
) -> Result<TerminalDto, ErrorDto> {
    let cwd = chat_id
        .and_then(|chat| state.workspace.roots(chat).ok())
        .and_then(|roots| roots.primary().map(PathBuf::from))
        .or_else(home_dir);
    let opened = state
        .terminals
        .open(
            &id,
            &Open { cwd, cols, rows },
            Arc::new(WindowSink(app)) as Arc<dyn TerminalSink>,
        )
        .map_err(refuse)?;
    Ok(TerminalDto {
        scrollback: state.terminals.scrollback(&id).unwrap_or_default(),
        id,
        shell: opened.shell,
        cwd: opened.cwd,
    })
}

/// Keystrokes, already encoded by the terminal emulator in the window: `\r` for Enter, `\x03`
/// for Ctrl+C, an escape sequence for an arrow key.
#[tauri::command]
#[specta::specta]
pub fn write_terminal(
    state: State<'_, AppState>,
    id: String,
    data: String,
) -> Result<(), ErrorDto> {
    state.terminals.write(&id, &data).map_err(refuse)
}

/// The tab changed size, in characters.
#[tauri::command]
#[specta::specta]
pub fn resize_terminal(
    state: State<'_, AppState>,
    id: String,
    cols: u16,
    rows: u16,
) -> Result<(), ErrorDto> {
    state.terminals.resize(&id, cols, rows).map_err(refuse)
}

/// The user closed the tab, which kills the shell. Closing a terminal that is already gone is
/// not an error: the tab closes either way.
#[tauri::command]
#[specta::specta]
pub fn close_terminal(state: State<'_, AppState>, id: String) {
    state.terminals.close(&id);
}

/// The user's home folder, as somewhere to start when the chat has no folder attached.
fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

fn refuse(err: gantry_terminal::TerminalError) -> ErrorDto {
    GantryError::Internal(err.to_string()).into()
}
