//! The terminal the user types in (`docs/plan/16 §5`).
//!
//! This is not the shell connector, and the difference is the whole point of the crate. The
//! connector runs one command line for the model with its input closed, its output captured and
//! a deadline on it (`docs/connectors/shell.md` D3) — which is what makes a tool call
//! reproducible, and what makes `vim`, `top` and a password prompt impossible. This is the other
//! thing: a real pseudo-terminal running the user's own login shell, with a keyboard attached to
//! it, for the moments when a person wants to run something themselves without leaving the
//! window they are working in.
//!
//! Nothing here is offered to a model. A terminal is opened by the user, typed in by the user,
//! and closed by the user; no tool reaches into one.
//!
//! One process per terminal, one reader thread per process. The thread owns the blocking read
//! and hands whole `&str`s to a sink — the app layer turns those into Tauri events — because a
//! pty read splits multi-byte characters as happily as it splits lines, and every consumer
//! downstream of here would otherwise have to know that.

#![forbid(unsafe_code)]

use std::{
    collections::HashMap,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};

/// What a terminal keeps of what it has printed, so a tab that is closed and opened again comes
/// back to the session it left rather than to a blank rectangle.
///
/// It is a byte count rather than a line count because that is what bounds the memory: one
/// `cat` of a minified file is a single line and a megabyte of it.
const SCROLLBACK_BYTES: usize = 256 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum TerminalError {
    #[error("this terminal is no longer open")]
    Gone,
    #[error("a terminal could not be started: {0}")]
    Spawn(String),
    #[error("the terminal could not be written to: {0}")]
    Io(String),
}

/// Where a terminal's output goes. Implemented by the app layer, which emits it to the window.
///
/// Called from the reader thread, so an implementation that blocks holds up the terminal it is
/// reporting on.
pub trait TerminalSink: Send + Sync + 'static {
    /// Text the terminal printed: escape sequences included, because the renderer is a terminal
    /// emulator and they are what make it one.
    fn output(&self, id: &str, data: &str);
    /// The shell exited. The terminal stays in the map until it is closed, so the last screen
    /// can still be read.
    fn exited(&self, id: &str, code: Option<i32>);
}

/// What to start, and how big the window is.
pub struct Open {
    /// Where the shell starts. The session's folder, and the home directory when it has none.
    pub cwd: Option<PathBuf>,
    pub cols: u16,
    pub rows: u16,
}

/// What was started, for the tab's header.
#[derive(Debug, Clone)]
pub struct Opened {
    /// The shell that is running, as a person names it: `zsh`, `bash`, `powershell`.
    pub shell: String,
    pub cwd: String,
}

struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    /// Shared with the reader thread, which waits on it at end-of-file so the tab can say
    /// what the shell exited with rather than only that it is gone.
    child: Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
    scrollback: Arc<Mutex<String>>,
    opened: Opened,
}

/// Every open terminal in this process.
#[derive(Default)]
pub struct Terminals {
    sessions: Mutex<HashMap<String, Session>>,
}

impl Terminals {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts a shell on a new pty under `id`, or returns what is already running under it.
    ///
    /// Re-opening is the ordinary case rather than the exception: the tab is a React component,
    /// and closing it, switching chats or reloading the window unmounts it while the shell goes
    /// on running. The caller reads [`Terminals::scrollback`] afterwards to redraw what it
    /// missed.
    pub fn open(
        &self,
        id: &str,
        open: &Open,
        sink: Arc<dyn TerminalSink>,
    ) -> Result<Opened, TerminalError> {
        {
            let sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(existing) = sessions.get(id) {
                return Ok(existing.opened.clone());
            }
        }

        let size = PtySize {
            rows: open.rows.max(1),
            cols: open.cols.max(1),
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = native_pty_system()
            .openpty(size)
            .map_err(|e| TerminalError::Spawn(e.to_string()))?;

        let (program, args) = login_shell();
        let mut cmd = CommandBuilder::new(&program);
        for arg in &args {
            cmd.arg(arg);
        }
        if let Some(cwd) = open.cwd.as_ref().filter(|p| p.is_dir()) {
            cmd.cwd(cwd);
        }
        // What a terminal promises the programs inside it. Without `TERM` a shell assumes a
        // dumb terminal and drops colour, line editing and the alternate screen — the three
        // things that make this worth having over the connector's captured output.
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| TerminalError::Spawn(e.to_string()))?;
        // The slave end has to go, or the pty never reports end-of-file when the shell exits
        // and the reader thread waits for ever on a terminal nobody is using.
        drop(pair.slave);

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| TerminalError::Spawn(e.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| TerminalError::Spawn(e.to_string()))?;

        let opened = Opened {
            shell: shell_name(&program),
            cwd: open
                .cwd
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
        };
        let scrollback = Arc::new(Mutex::new(String::new()));
        let child = Arc::new(Mutex::new(child));
        spawn_reader(
            id.to_owned(),
            reader,
            Arc::clone(&scrollback),
            Arc::clone(&child),
            sink,
        );

        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        sessions.insert(
            id.to_owned(),
            Session {
                master: pair.master,
                writer,
                child,
                scrollback,
                opened: opened.clone(),
            },
        );
        Ok(opened)
    }

    /// Keystrokes, as the terminal emulator encoded them. Every key is a byte sequence here:
    /// Enter is `\r`, Ctrl+C is `\x03`, and an arrow key is an escape sequence, which is why
    /// this takes text from the front end rather than a key name.
    pub fn write(&self, id: &str, data: &str) -> Result<(), TerminalError> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        let session = sessions.get_mut(id).ok_or(TerminalError::Gone)?;
        session
            .writer
            .write_all(data.as_bytes())
            .and_then(|()| session.writer.flush())
            .map_err(|e| TerminalError::Io(e.to_string()))
    }

    /// The window changed shape. Programs that draw a screen are told by `SIGWINCH`, which is
    /// the difference between `top` redrawing and `top` wrapping every line.
    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<(), TerminalError> {
        let sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        let session = sessions.get(id).ok_or(TerminalError::Gone)?;
        session
            .master
            .resize(PtySize {
                rows: rows.max(1),
                cols: cols.max(1),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| TerminalError::Io(e.to_string()))
    }

    /// What this terminal has printed, for a tab that is being drawn again.
    #[must_use]
    pub fn scrollback(&self, id: &str) -> Option<String> {
        let sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        sessions.get(id).map(|s| {
            s.scrollback
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
        })
    }

    #[must_use]
    pub fn is_open(&self, id: &str) -> bool {
        let sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        sessions.contains_key(id)
    }

    /// Closes the terminal and kills the shell. The user closed the tab; a shell left running
    /// with nothing attached to it is a leak, not a feature.
    pub fn close(&self, id: &str) {
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(session) = sessions.remove(id) {
            let mut child = session.child.lock().unwrap_or_else(|e| e.into_inner());
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// Every terminal, closed. Called when the window goes away.
    pub fn close_all(&self) {
        let ids: Vec<String> = {
            let sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
            sessions.keys().cloned().collect()
        };
        for id in ids {
            self.close(&id);
        }
    }
}

/// Reads the pty until it ends, handing whole strings to the sink and keeping the tail.
fn spawn_reader(
    id: String,
    mut reader: Box<dyn Read + Send>,
    scrollback: Arc<Mutex<String>>,
    child: Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
    sink: Arc<dyn TerminalSink>,
) {
    let name = format!("terminal-{id}");
    std::thread::Builder::new()
        .name(name.clone())
        .spawn(move || {
            let mut chunk = [0_u8; 8 * 1024];
            let mut decoder = Utf8Stream::default();
            loop {
                match reader.read(&mut chunk) {
                    // End of file: the shell exited and the pty closed behind it.
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let text = decoder.push(&chunk[..n]);
                        if text.is_empty() {
                            continue;
                        }
                        {
                            let mut kept = scrollback.lock().unwrap_or_else(|e| e.into_inner());
                            kept.push_str(&text);
                            trim_scrollback(&mut kept);
                        }
                        sink.output(&id, &text);
                    }
                }
            }
            // The pty is closed, so the shell is on its way out; `wait` is what turns that
            // into the number the tab prints.
            let code = child
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .wait()
                .ok()
                .map(|status| i32::try_from(status.exit_code()).unwrap_or(-1));
            sink.exited(&id, code);
        })
        .map(|_| ())
        .unwrap_or_else(|e| log::warn!("{name}: no reader thread: {e}"));
}

/// Keeps the tail of the scrollback, cut at a character boundary and then at the first newline
/// after it, so what comes back starts at a whole line rather than mid-escape.
fn trim_scrollback(kept: &mut String) {
    if kept.len() <= SCROLLBACK_BYTES {
        return;
    }
    let mut from = kept.len() - SCROLLBACK_BYTES;
    while from < kept.len() && !kept.is_char_boundary(from) {
        from += 1;
    }
    let from = kept[from..]
        .find('\n')
        .map_or(from, |at| from + at + 1)
        .min(kept.len());
    kept.drain(..from);
}

/// A byte stream decoded as it arrives.
///
/// A pty hands over whatever the program wrote since the last read, which cuts multi-byte
/// characters in half about as often as anything else: an 8 KB read of UTF-8 ends mid-character
/// roughly three times in four. Decoding each read on its own would put a replacement character
/// in the middle of every other emoji and box-drawing glyph, so the tail that is not yet a
/// character waits here for the rest of itself.
#[derive(Default)]
struct Utf8Stream {
    partial: Vec<u8>,
}

impl Utf8Stream {
    fn push(&mut self, bytes: &[u8]) -> String {
        self.partial.extend_from_slice(bytes);
        let mut out = String::new();
        loop {
            match std::str::from_utf8(&self.partial) {
                Ok(text) => {
                    out.push_str(text);
                    self.partial.clear();
                    return out;
                }
                Err(err) => {
                    let valid = err.valid_up_to();
                    // `from_utf8` has already proved this prefix, so the conversion cannot fail.
                    out.push_str(std::str::from_utf8(&self.partial[..valid]).unwrap_or_default());
                    match err.error_len() {
                        // Genuinely invalid bytes: not the beginning of anything, so they are
                        // replaced rather than waited on. A terminal prints binary sometimes.
                        Some(bad) => {
                            out.push(char::REPLACEMENT_CHARACTER);
                            self.partial.drain(..valid + bad);
                        }
                        // The tail is the start of a character whose rest has not arrived.
                        None => {
                            self.partial.drain(..valid);
                            return out;
                        }
                    }
                }
            }
        }
    }
}

/// The user's own shell, interactive. It reads their startup files — unlike the connector's
/// shell, which deliberately does not (`shell.md` D2): the aliases and functions a person has
/// set up are noise to a classifier and the entire point to a person typing.
fn login_shell() -> (PathBuf, Vec<String>) {
    #[cfg(windows)]
    {
        // PowerShell where it exists, in the order a Windows user would expect to get it.
        for candidate in ["pwsh.exe", "powershell.exe"] {
            if which(candidate).is_some() {
                return (PathBuf::from(candidate), Vec::new());
            }
        }
        (PathBuf::from("cmd.exe"), Vec::new())
    }
    #[cfg(not(windows))]
    {
        let shell = std::env::var_os("SHELL")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| PathBuf::from("/bin/sh"));
        // No `-l`: the shell has a terminal, so it is interactive by itself, and a login shell
        // would re-run the profile that the app's own environment already came from.
        (shell, Vec::new())
    }
}

/// Whether a program is on the path, for the Windows shell choice.
#[cfg(windows)]
fn which(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

/// `/bin/zsh` as `zsh`, `powershell.exe` as `powershell`.
fn shell_name(program: &Path) -> String {
    program
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| program.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_character_split_across_reads_is_not_mangled() {
        let mut stream = Utf8Stream::default();
        // "é" is two bytes, and the pty handed over one of them.
        let bytes = "aé".as_bytes();
        assert_eq!(stream.push(&bytes[..2]), "a");
        assert_eq!(stream.push(&bytes[2..]), "é");
    }

    #[test]
    fn an_escape_sequence_survives_the_decoder() {
        let mut stream = Utf8Stream::default();
        assert_eq!(stream.push(b"\x1b[31mred\x1b[0m"), "\x1b[31mred\x1b[0m");
    }

    #[test]
    fn bytes_that_are_not_utf8_become_one_replacement_and_do_not_stall() {
        let mut stream = Utf8Stream::default();
        assert_eq!(stream.push(&[b'a', 0xff, b'b']), "a\u{fffd}b");
        assert!(stream.partial.is_empty(), "nothing is left waiting");
    }

    #[test]
    fn scrollback_is_cut_to_a_whole_line() {
        let mut kept = String::new();
        for n in 0..40_000 {
            kept.push_str(&format!("line {n}\n"));
        }
        trim_scrollback(&mut kept);
        assert!(kept.len() <= SCROLLBACK_BYTES);
        assert!(
            kept.starts_with("line "),
            "the tail starts at a line, not mid-word: {:?}",
            &kept[..20]
        );
    }

    #[test]
    fn the_shell_has_a_name_a_person_would_recognise() {
        assert_eq!(shell_name(Path::new("/bin/zsh")), "zsh");
        assert_eq!(shell_name(Path::new("powershell.exe")), "powershell");
    }
}
