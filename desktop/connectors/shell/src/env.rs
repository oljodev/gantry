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

use std::{collections::BTreeMap, ffi::OsString, path::PathBuf, time::Duration};

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
            program,
            args,
            label,
            login_label: shell_name(&login_shell()),
            vars: std::env::vars().collect(),
            from_login_shell: false,
        }
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
            OsString::from("-Command"),
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
