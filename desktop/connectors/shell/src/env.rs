//! The environment a command gets (`docs/connectors/shell.md` D2, §3).
//!
//! A graphical application on macOS inherits a nearly empty `PATH`: launched from Finder or a
//! `.app`, it never sees `~/.zshrc`, so `node`, `cargo` and everything a version manager puts on
//! the path are simply missing. The fix is to ask the login shell once, at startup, what it has —
//! and then to run individual commands in a shell that does *not* re-read startup files, so the
//! user's profile executes once per app run rather than once per `git status`.
//!
//! That second half is also what gives the classifier its meaning: a shell that re-read the
//! user's aliases and functions on every call could resolve `ls` to anything at all.
//!
//! **Which shell runs the command is a different question from which shell has the
//! environment** (decided 2026-09-08, correcting shell.md §3, which said the login shell did
//! both). Models write POSIX command lines — `a && b`, `2>&1`, `VAR=x cmd` — and a user whose
//! login shell is fish, nushell or xonsh would have almost every one of them fail on syntax
//! rather than on merit. So the login shell is asked for its environment, and the command runs
//! in bash where it exists and `/bin/sh` otherwise. The shell that ran is reported in the
//! result, because "it worked in my terminal" and "it worked in Gantry" differing by shell is
//! otherwise a mystery.

use std::{collections::BTreeMap, ffi::OsString, path::PathBuf};
// Only the unix capture waits on a login shell; on Windows there is nothing to wait for.
#[cfg(unix)]
use std::time::Duration;

/// What every command runs with: a POSIX shell, and the login shell's environment.
#[derive(Debug, Clone)]
pub struct ShellEnv {
    /// The program that runs command lines, and its "run this string" flag.
    pub program: PathBuf,
    pub args: Vec<OsString>,
    /// A human name for the result, so "it worked in my terminal" can be compared.
    pub label: String,
    /// The user's login shell, which supplied the environment. Only the same as `label` when
    /// their login shell is also the one commands run in.
    pub login_label: String,
    pub vars: BTreeMap<String, String>,
    /// True when the login shell answered; false when this is the process environment.
    pub from_login_shell: bool,
    /// True when command lines are handed to PowerShell rather than to a POSIX shell. It
    /// changes two things: how the command text is passed (see [`ShellEnv::command_arg`]) and
    /// what the model is told about writing one.
    pub powershell: bool,
}

impl ShellEnv {
    /// Ask the login shell for its environment. Called once, at startup.
    ///
    /// The capture has a short timeout because a broken profile that blocks would otherwise
    /// hang the app's start; failing back to the process environment gives a working shell with
    /// a poorer `PATH`, which is worse than the real thing and much better than nothing.
    #[must_use]
    pub fn capture() -> Self {
        let mut env = Self::inherited();
        if let Some(vars) = capture_login_vars(&login_shell()) {
            env.vars = vars;
            env.from_login_shell = true;
        }
        env
    }

    /// The process environment and the platform's shell, with no login capture. Used as the
    /// fallback above, and by tests, which must not depend on the developer's profile.
    #[must_use]
    pub fn inherited() -> Self {
        let (program, args, label) = shell_program();
        Self {
            powershell: cfg!(windows),
            program,
            args,
            label,
            login_label: shell_name(&login_shell()),
            vars: std::env::vars().collect(),
            from_login_shell: false,
        }
    }

    /// The last argument the shell is given: the command line itself.
    ///
    /// On a POSIX shell that is the text, because `bash -c` takes it as one argument. **On
    /// PowerShell it is base64**, because `-Command` does not: PowerShell re-parses everything
    /// after it with its own rules, while Rust quotes an argument with the C runtime's, and the
    /// two disagree about the backslash and the quote. `git commit -m "fix: thing"` reaches the
    /// shell as `git commit -m \"fix: thing\"` and commits a message with backslashes in it.
    /// `-EncodedCommand` has no quoting to disagree about (see [`powershell_encoded`]).
    #[must_use]
    pub fn command_arg(&self, command: &str) -> OsString {
        if self.powershell {
            OsString::from(powershell_encoded(command))
        } else {
            OsString::from(command)
        }
    }

    /// What the model is told about writing a command line for *this* shell, appended to the
    /// tool's description.
    ///
    /// Without it a model writes POSIX on every platform, which on Windows means `&&` (a syntax
    /// error before PowerShell 7), `VAR=x cmd` (not a thing at all) and `ls -la` (an alias that
    /// takes different flags). The shell is not a detail the caller can be left to guess.
    #[must_use]
    pub fn note_for_model(&self) -> String {
        if self.powershell {
            format!(
                "\n\nCommand lines here are run by PowerShell ({}), not by a POSIX shell. \
                 Separate commands with `;` — `&&` and `||` need PowerShell 7 and are a syntax \
                 error in Windows PowerShell 5. Set a variable for one command with \
                 `$env:NAME = 'value'`, not `NAME=value cmd`. Where a Unix program is not \
                 installed, use PowerShell's own (`Get-ChildItem`, `Select-String`, \
                 `Remove-Item`). The profile is not read, so nothing defined in it exists here.",
                self.label
            )
        } else {
            format!(
                "\n\nCommand lines here are run by {} with `-c`. The profile is not read, so \
                 aliases and functions defined in it are not available.",
                self.label
            )
        }
    }

    /// The folder a command runs in when the chat has no folder attached: the one a terminal
    /// opens in. Taken from the captured environment rather than from this process, so it is the
    /// home the user's shell would use. `None` when there is no such directory, which is a state
    /// worth reporting rather than papering over with a guess.
    #[must_use]
    pub fn home(&self) -> Option<PathBuf> {
        let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
        let path = self
            .vars
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| PathBuf::from(v))?;
        path.is_dir().then_some(path)
    }

    /// The `PATH` a command will see, for the "program not found" message.
    #[must_use]
    pub fn path(&self) -> Option<&str> {
        self.vars
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("PATH"))
            .map(|(_, v)| v.as_str())
    }
}

/// The user's login shell, which is asked for the environment and nothing else.
#[cfg(unix)]
fn login_shell() -> PathBuf {
    std::env::var_os("SHELL").map_or_else(|| PathBuf::from("/bin/sh"), PathBuf::from)
}

#[cfg(unix)]
fn shell_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "sh".to_owned())
}

/// The shell command lines run in: bash where it exists, `/bin/sh` otherwise. Not the login
/// shell — see the note at the top of this file.
#[cfg(unix)]
fn shell_program() -> (PathBuf, Vec<OsString>, String) {
    let bash = ["/bin/bash", "/usr/bin/bash", "/opt/homebrew/bin/bash"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.is_file());
    let program = bash.unwrap_or_else(|| PathBuf::from("/bin/sh"));
    let label = shell_name(&program);
    // `-c` only: no `-l`, no `-i`. The profile ran once in `capture`.
    (program, vec![OsString::from("-c")], label)
}

#[cfg(windows)]
fn shell_program() -> (PathBuf, Vec<OsString>, String) {
    // PowerShell 7 where it exists, Windows PowerShell otherwise. `-NoProfile` is the same
    // decision as not passing `-l` on unix, and `-NonInteractive` keeps a prompt from waiting
    // for a person who is not there.
    let seven = which("pwsh.exe");
    let (program, label) = match seven {
        Some(path) => (path, "pwsh".to_owned()),
        None => (PathBuf::from("powershell.exe"), "powershell".to_owned()),
    };
    (
        program,
        vec![
            OsString::from("-NoProfile"),
            OsString::from("-NonInteractive"),
            // Not `-Command`: see `ShellEnv::command_arg`.
            OsString::from("-EncodedCommand"),
        ],
        label,
    )
}

#[cfg(windows)]
fn which(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

/// Run the login shell once and read back its environment.
#[cfg(unix)]
fn capture_login_vars(shell: &std::path::Path) -> Option<BTreeMap<String, String>> {
    use std::process::{Command, Stdio};

    // `-l` reads the profile; `env -0` prints NUL-separated pairs, which survive values with
    // newlines in them (a `PS1` or a `LS_COLORS` will have some).
    // `-l` and `-c` as separate arguments: the combined `-lc` is a bash-ism, and the shell being
    // asked here is whatever the user logs in with — fish, nushell, anything.
    let mut child = Command::new(shell)
        .arg("-l")
        .arg("-c")
        .arg("env -0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let output = wait_with_timeout(&mut child, Duration::from_secs(5))?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let vars: BTreeMap<String, String> = text
        .split('\0')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
    (!vars.is_empty()).then_some(vars)
}

#[cfg(windows)]
fn login_shell() -> PathBuf {
    PathBuf::from("powershell.exe")
}

#[cfg(windows)]
fn shell_name(path: &std::path::Path) -> String {
    path.file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "powershell".to_owned())
}

#[cfg(windows)]
fn capture_login_vars(_shell: &std::path::Path) -> Option<BTreeMap<String, String>> {
    // Windows has no login-shell environment separate from the process environment: a service
    // or a GUI app already inherits the user's `PATH` from the registry.
    None
}

#[cfg(unix)]
fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
) -> Option<std::process::Output> {
    use std::{io::Read, thread, time::Instant};

    let mut stdout = child.stdout.take()?;
    let reader = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = reader.join().unwrap_or_default();
                return Some(std::process::Output {
                    status,
                    stdout,
                    stderr: Vec::new(),
                });
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            // A profile that hangs is a profile Gantry does not wait for.
            _ => {
                let _ = child.kill();
                return None;
            }
        }
    }
}

/// The script as PowerShell's `-EncodedCommand` wants it: base64 of UTF-16, little-endian.
///
/// It carries a prelude of its own. PowerShell writes its output in the console's code page,
/// which on a Norwegian machine is not UTF-8, so `æ` reaches the model as mojibake unless the
/// script says otherwise. `UTF8Encoding::new($false)` rather than `[Text.Encoding]::UTF8`
/// because the latter writes a byte-order mark, which would arrive as three stray characters at
/// the top of every command's output.
///
/// Compiled everywhere and tested everywhere, though it is only used on Windows: an encoder
/// nothing runs is an encoder nothing checks.
#[must_use]
pub fn powershell_encoded(command: &str) -> String {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let script = format!("{PS_PRELUDE}{command}");
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    STANDARD.encode(utf16)
}

const PS_PRELUDE: &str =
    "$OutputEncoding = [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)\n";

/// A [`ShellEnv`] being captured on another thread, waited for by the first command that needs
/// it (`docs/connectors/shell.md` D2).
///
/// The capture runs the user's login shell, which reads their profile: on a machine with a
/// version manager and a prompt framework that is a few hundred milliseconds, and the timeout
/// above allows five seconds of it. None of that belongs in front of a window, and nothing
/// before the first command needs the answer — so the shell is asked at startup and the answer
/// is collected when it is first read, which is usually long after it arrived.
pub struct PendingShellEnv {
    ready: std::sync::OnceLock<ShellEnv>,
    pending: std::sync::Mutex<Option<std::thread::JoinHandle<ShellEnv>>>,
}

impl PendingShellEnv {
    /// Starts the capture and returns at once.
    #[must_use]
    pub fn capture() -> Self {
        Self {
            ready: std::sync::OnceLock::new(),
            pending: std::sync::Mutex::new(Some(std::thread::spawn(ShellEnv::capture))),
        }
    }

    /// An environment that is already known. For tests, and for the places that build a shell
    /// without wanting the user's profile in it.
    #[must_use]
    pub fn ready(env: ShellEnv) -> Self {
        let ready = std::sync::OnceLock::new();
        let _ = ready.set(env);
        Self {
            ready,
            pending: std::sync::Mutex::new(None),
        }
    }

    /// The environment, waiting for the capture if it is still running. A capture thread that
    /// panicked leaves the process environment, which is what a failed capture leaves anyway.
    pub fn get(&self) -> &ShellEnv {
        if let Some(env) = self.ready.get() {
            return env;
        }
        let taken = self
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(handle) = taken {
            let _ = self
                .ready
                .set(handle.join().unwrap_or_else(|_| ShellEnv::inherited()));
        }
        self.ready.get_or_init(ShellEnv::inherited)
    }
}

impl std::fmt::Debug for PendingShellEnv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.ready.get() {
            Some(env) => write!(f, "PendingShellEnv({env:?})"),
            None => f.write_str("PendingShellEnv(capturing)"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_inherited_environment_has_a_shell_and_a_path() {
        let env = ShellEnv::inherited();
        assert!(!env.label.is_empty());
        assert!(env.path().is_some(), "the test process has a PATH");
        assert!(!env.from_login_shell);
    }

    #[cfg(unix)]
    #[test]
    fn commands_run_without_reading_the_profile() {
        let env = ShellEnv::inherited();
        assert_eq!(env.args, vec![OsString::from("-c")], "no -l, no -i");
    }

    #[test]
    fn the_home_folder_comes_from_the_captured_environment() {
        let mut env = ShellEnv::inherited();
        let dir = tempfile::tempdir().unwrap();
        let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
        env.vars
            .insert(key.to_owned(), dir.path().display().to_string());
        assert_eq!(env.home().as_deref(), Some(dir.path()));

        env.vars.insert(
            key.to_owned(),
            dir.path().join("gone").display().to_string(),
        );
        assert!(
            env.home().is_none(),
            "a home that is not there is not a home"
        );
    }

    /// The Windows path, exercised from Linux — which is the only place it *is* exercised
    /// until somebody builds for Windows.
    #[test]
    fn powershell_is_handed_its_script_rather_than_an_argument() {
        let command = r#"git commit -m "fix: the thing" && echo 'done'"#;
        let encoded = powershell_encoded(command);

        // Base64 of UTF-16LE: decode it back the way PowerShell will.
        use base64::{Engine, engine::general_purpose::STANDARD};
        let bytes = STANDARD.decode(&encoded).expect("valid base64");
        assert_eq!(bytes.len() % 2, 0, "UTF-16 comes in pairs");
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|p| u16::from_le_bytes([p[0], p[1]]))
            .collect();
        let script = String::from_utf16(&units).expect("valid UTF-16");

        // The quotes arrive exactly as the model wrote them. This is the whole point: through
        // `-Command` they would have reached PowerShell with backslashes in front of them.
        assert!(script.ends_with(command), "{script}");
        assert!(
            script.starts_with("$OutputEncoding"),
            "the script sets its own output encoding first: {script}"
        );
        assert!(
            !script.contains(r#"\""#),
            "nothing escaped anything: {script}"
        );
    }

    #[test]
    fn a_posix_shell_is_handed_the_command_unchanged() {
        let env = ShellEnv::inherited();
        if env.powershell {
            return; // This test runs the other way round on Windows.
        }
        let command = "echo 'hello' && ls -la";
        assert_eq!(env.command_arg(command), OsString::from(command));
    }

    #[test]
    fn the_model_is_told_which_shell_it_is_writing_for() {
        let mut env = ShellEnv::inherited();
        env.powershell = true;
        env.label = "pwsh".to_owned();
        let note = env.note_for_model();
        assert!(note.contains("PowerShell (pwsh)"), "{note}");
        assert!(note.contains("$env:NAME"), "{note}");

        env.powershell = false;
        env.label = "bash".to_owned();
        let note = env.note_for_model();
        assert!(note.contains("bash"), "{note}");
        assert!(!note.contains("PowerShell"), "{note}");
    }

    #[cfg(unix)]
    #[test]
    fn commands_run_in_a_posix_shell_whatever_the_user_logs_in_with() {
        // A user whose login shell is fish still gets `a && b` and `2>&1` working, because the
        // login shell supplies the environment and not the syntax.
        let env = ShellEnv::inherited();
        assert!(
            matches!(env.label.as_str(), "bash" | "sh"),
            "commands ran in {}",
            env.label
        );
    }
}
