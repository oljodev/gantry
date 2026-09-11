//! The runtime check of `docs/plan/03-connector-system.md` §11, step 1.
//!
//! A local MCP server is a program that runs on something else — Node, uv, Docker — and the
//! install flow's first step is finding out whether that something is here. The rule the plan
//! states and this enforces is that there is no "install anyway": a server whose runtime is
//! missing cannot be installed, because the alternative is an instance in the list that fails on
//! every call with an error about `npx` that means nothing to the person reading it.
//!
//! What counts as "here" is the *login shell's* `PATH`, not the app's (01 §6). A desktop
//! application launched from a dock inherits almost nothing, and every version manager there is —
//! nvm, asdf, mise, volta, pyenv — puts its shims on the `PATH` a shell builds and nowhere else.
//! Looking in the process environment would report Node missing on the machine of everybody who
//! installed Node the normal way.

use std::{collections::BTreeMap, path::PathBuf, time::Duration};

use gantry_core::RuntimeRequirement;
use serde::{Deserialize, Serialize};

/// How long one `--version` may take. A runtime manager's shim can be slow the first time; a
/// program that has not answered in this long is not going to.
const TIMEOUT: Duration = Duration::from_secs(10);

/// What one requirement resolves to on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct RuntimeStatus {
    /// `node`, `python`, `uv`, `docker`.
    pub name: String,
    /// The range the manifest asks for, exactly as it wrote it.
    pub required: String,
    /// The version found, when the program answered.
    pub found: Option<String>,
    /// Where it was found, so "but I have Node" can be checked rather than argued with.
    pub path: Option<String>,
    pub ok: bool,
    /// Why not, in a sentence the person reading it can act on.
    pub problem: Option<String>,
    /// How to get it on this operating system.
    pub install: Vec<InstallHint>,
}

/// One way to install a runtime: a command to paste, a page to open, or both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct InstallHint {
    /// "Homebrew", "winget", "Download from nodejs.org".
    pub label: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

/// Checks every requirement, in parallel because each one is a process start.
pub async fn detect(
    requirements: &[RuntimeRequirement],
    env: &BTreeMap<String, String>,
) -> Vec<RuntimeStatus> {
    let futures = requirements.iter().map(|req| check(req, env));
    futures_util::future::join_all(futures).await
}

async fn check(req: &RuntimeRequirement, env: &BTreeMap<String, String>) -> RuntimeStatus {
    let mut status = RuntimeStatus {
        name: req.name.clone(),
        required: req.version.clone(),
        found: None,
        path: None,
        ok: false,
        problem: None,
        install: hints(&req.name),
    };
    let Some(program) = which(&req.name, env) else {
        status.problem = Some(format!("{} is not on your PATH", label(&req.name)));
        return status;
    };
    status.path = Some(program.display().to_string());
    let Some(version) = ask_version(&program, &req.name, env).await else {
        // It is there and it would not say what it is. Installing on top of that is a guess.
        status.problem = Some(format!(
            "{} is at {} but did not answer `--version`",
            label(&req.name),
            program.display()
        ));
        return status;
    };
    let satisfied = satisfies(&version, &req.version);
    status.found = Some(version.clone());
    status.ok = satisfied;
    if !satisfied {
        status.problem = Some(format!(
            "{} {version} is installed; this connector needs {}",
            label(&req.name),
            req.version
        ));
    }
    status
}

/// The first entry on the `PATH` holding an executable of this name.
///
/// Written out rather than taken from a crate because the `PATH` searched is the one captured
/// from the login shell, not this process's, and every `which` crate reads the latter.
fn which(name: &str, env: &BTreeMap<String, String>) -> Option<PathBuf> {
    let path = env
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("PATH"))
        .map(|(_, v)| v.as_str())?;
    let exts: Vec<String> = if cfg!(windows) {
        env.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("PATHEXT"))
            .map(|(_, v)| v.split(';').map(str::to_ascii_lowercase).collect())
            .unwrap_or_else(|| vec![".exe".to_owned(), ".cmd".to_owned(), ".bat".to_owned()])
    } else {
        vec![String::new()]
    };
    let separator = if cfg!(windows) { ';' } else { ':' };
    for candidate in programs(name) {
        for dir in path.split(separator).filter(|d| !d.is_empty()) {
            for ext in &exts {
                let file = PathBuf::from(dir).join(format!("{candidate}{ext}"));
                if file.is_file() {
                    return Some(file);
                }
            }
        }
    }
    None
}

/// What a requirement is actually called on disk. `python` is the one that matters: on most
/// machines `python3` exists and `python` either does not or is the wrong one.
fn programs(name: &str) -> Vec<&str> {
    match name {
        "python" => vec!["python3", "python"],
        other => vec![other],
    }
}

async fn ask_version(
    program: &PathBuf,
    name: &str,
    env: &BTreeMap<String, String>,
) -> Option<String> {
    let mut command = tokio::process::Command::new(program);
    command.arg("--version").env_clear().envs(env);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let output = tokio::time::timeout(TIMEOUT, command.output())
        .await
        .ok()?
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let text = if text.trim().is_empty() {
        String::from_utf8_lossy(&output.stderr).into_owned()
    } else {
        text.into_owned()
    };
    let _ = name;
    first_version(&text)
}

/// The first dotted number in a `--version` line. Every runtime prints a different sentence
/// around it — `v22.1.0`, `Python 3.12.1`, `uv 0.4.18`, `Docker version 27.1.1, build ab12cd` —
/// and all of them put the version in the same shape.
#[must_use]
pub fn first_version(text: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let found: String = chars[start..i].iter().collect();
            let found = found.trim_end_matches('.').to_owned();
            if !found.is_empty() {
                return Some(found);
            }
        }
        i += 1;
    }
    None
}

/// Whether a found version satisfies a manifest's range.
///
/// The ranges a manifest may write are deliberately few: `>=20`, `>20`, `=20.1`, `20`, `*`. A
/// bare number means "this or newer", which is what every manifest that writes one means, and
/// anything unrecognised is treated as satisfied rather than blocking an install over a range
/// this cannot read — the runtime is there, and refusing on a parse failure would be Gantry's
/// bug punishing the user.
#[must_use]
pub fn satisfies(found: &str, range: &str) -> bool {
    let range = range.trim();
    if range.is_empty() || range == "*" {
        return true;
    }
    let (op, wanted) = if let Some(rest) = range.strip_prefix(">=") {
        (">=", rest)
    } else if let Some(rest) = range.strip_prefix('>') {
        (">", rest)
    } else if let Some(rest) = range.strip_prefix("==") {
        ("=", rest)
    } else if let Some(rest) = range.strip_prefix('=') {
        ("=", rest)
    } else if range.starts_with(|c: char| c.is_ascii_digit()) {
        (">=", range)
    } else {
        return true;
    };
    let (Some(found), Some(wanted)) = (parts(found), parts(wanted.trim())) else {
        return true;
    };
    match op {
        // Ordering compares the components the two versions have in common and then their
        // length, so `20` and `20.0.0` have to be the same length to be equal.
        ">" => padded(&found) > padded(&wanted),
        // `=20` means any 20.x. A manifest that writes fewer components is asking about fewer
        // components, so only the ones it wrote are compared.
        "=" => found.len() >= wanted.len() && found[..wanted.len()] == wanted[..],
        _ => padded(&found) >= padded(&wanted),
    }
}

/// A dotted version as numbers, exactly as many as were written.
fn parts(version: &str) -> Option<Vec<u64>> {
    let mut out: Vec<u64> = Vec::new();
    for piece in version.split('.') {
        let digits: String = piece.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() {
            break;
        }
        out.push(digits.parse().ok()?);
    }
    (!out.is_empty()).then_some(out)
}

/// The same version to a fixed width, so `20` and `20.0.0` order equal.
fn padded(version: &[u64]) -> Vec<u64> {
    let mut out = version.to_vec();
    out.resize(4, 0);
    out
}

fn label(name: &str) -> &str {
    match name {
        "node" => "Node.js",
        "python" => "Python",
        "uv" => "uv",
        "docker" => "Docker",
        other => other,
    }
}

/// How to get a runtime on this operating system.
///
/// A package manager first where one is the normal answer, and the vendor's own page last
/// because it always works. Linux gets no `apt` line for Node on purpose: every distribution
/// ships a different major version and most of them ship one too old for the servers that ask
/// for Node, so pointing at apt would mean "install this, then fail this check again".
#[must_use]
pub fn hints(name: &str) -> Vec<InstallHint> {
    let hint = |label: &str, command: Option<&str>, url: Option<&str>| InstallHint {
        label: label.to_owned(),
        command: command.map(str::to_owned),
        url: url.map(str::to_owned),
    };
    match name {
        "node" if cfg!(target_os = "macos") => vec![
            hint("Homebrew", Some("brew install node"), None),
            hint("Download", None, Some("https://nodejs.org/en/download")),
        ],
        "node" if cfg!(windows) => vec![
            hint("winget", Some("winget install OpenJS.NodeJS.LTS"), None),
            hint("Download", None, Some("https://nodejs.org/en/download")),
        ],
        "node" => vec![
            hint(
                "nvm",
                Some(
                    "curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.1/install.sh | bash && nvm install --lts",
                ),
                None,
            ),
            hint("Download", None, Some("https://nodejs.org/en/download")),
        ],
        "uv" if cfg!(windows) => vec![
            hint("winget", Some("winget install astral-sh.uv"), None),
            hint(
                "Download",
                None,
                Some("https://docs.astral.sh/uv/getting-started/installation/"),
            ),
        ],
        "uv" => vec![
            hint(
                "Installer",
                Some("curl -LsSf https://astral.sh/uv/install.sh | sh"),
                None,
            ),
            hint("Homebrew", Some("brew install uv"), None),
        ],
        "python" if cfg!(target_os = "macos") => vec![
            hint("Homebrew", Some("brew install python"), None),
            hint("Download", None, Some("https://www.python.org/downloads/")),
        ],
        "python" if cfg!(windows) => vec![
            hint("winget", Some("winget install Python.Python.3.12"), None),
            hint("Download", None, Some("https://www.python.org/downloads/")),
        ],
        "python" => vec![hint(
            "Download",
            None,
            Some("https://www.python.org/downloads/"),
        )],
        "docker" if cfg!(target_os = "macos") => vec![
            hint("Homebrew", Some("brew install --cask docker"), None),
            hint(
                "Download",
                None,
                Some("https://www.docker.com/products/docker-desktop/"),
            ),
        ],
        "docker" => vec![hint(
            "Download",
            None,
            Some("https://www.docker.com/products/docker-desktop/"),
        )],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_is_found_in_whatever_sentence_it_arrives_in() {
        assert_eq!(first_version("v22.1.0\n").as_deref(), Some("22.1.0"));
        assert_eq!(first_version("Python 3.12.1").as_deref(), Some("3.12.1"));
        assert_eq!(
            first_version("uv 0.4.18 (abc 2024-01-01)").as_deref(),
            Some("0.4.18")
        );
        assert_eq!(
            first_version("Docker version 27.1.1, build ab12cd").as_deref(),
            Some("27.1.1")
        );
        assert_eq!(first_version("no idea").as_deref(), None);
    }

    #[test]
    fn a_bare_number_means_this_or_newer() {
        assert!(satisfies("22.1.0", "20"));
        assert!(satisfies("20.0.0", "20"));
        assert!(!satisfies("18.20.4", "20"));
        assert!(satisfies("22.1.0", ">=20"));
        assert!(!satisfies("20.0.0", ">20"));
        assert!(satisfies("20.0.1", ">20"));
    }

    #[test]
    fn a_pinned_major_accepts_its_own_minors() {
        assert!(satisfies("20.11.1", "=20"));
        assert!(!satisfies("21.0.0", "=20"));
        assert!(satisfies("3.12.1", "=3.12"));
        assert!(!satisfies("3.11.9", "=3.12"));
    }

    /// A range this cannot read is Gantry's gap, not the user's, and blocking an install over it
    /// would be the app refusing to work because of its own limitation.
    #[test]
    fn an_unreadable_range_does_not_block_an_install() {
        assert!(satisfies("22.1.0", "^20 || ^22"));
        assert!(satisfies("22.1.0", "*"));
        assert!(satisfies("22.1.0", ""));
    }

    #[test]
    fn python3_is_looked_for_before_python() {
        assert_eq!(programs("python"), vec!["python3", "python"]);
        assert_eq!(programs("node"), vec!["node"]);
    }

    /// The three pieces together, against a program on a `PATH` this test wrote: found, asked
    /// what it is, and compared. Unix only because it needs an executable, and a shell script is
    /// the cheapest one to write.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_runtime_on_the_given_path_is_found_and_asked_what_it_is() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("gantry-runtime-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let program = dir.join("node");
        std::fs::write(&program, "#!/bin/sh\necho v22.1.0\n").unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let env = BTreeMap::from([("PATH".to_owned(), dir.display().to_string())]);

        let asked = |version: &str| RuntimeRequirement {
            name: "node".to_owned(),
            version: version.to_owned(),
        };
        let found = detect(&[asked(">=20")], &env).await;
        assert!(found[0].ok);
        assert_eq!(found[0].found.as_deref(), Some("22.1.0"));
        assert_eq!(
            found[0].path.as_deref(),
            Some(program.display().to_string()).as_deref()
        );

        let short = detect(&[asked(">=24")], &env).await;
        assert!(!short[0].ok);
        assert!(short[0].problem.as_ref().unwrap().contains("22.1.0"));
        // A runtime that is there but too old still tells the user where to get a newer one.
        assert!(!short[0].install.is_empty());

        // The `PATH` searched is the one it was handed, not the process's: an empty one finds
        // nothing even on a machine with Node installed, which is the whole reason this takes
        // the login shell's environment rather than reading its own.
        let nowhere = detect(&[asked(">=20")], &BTreeMap::new()).await;
        assert!(!nowhere[0].ok && nowhere[0].found.is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn every_runtime_a_manifest_may_ask_for_can_be_installed_from_the_dialog() {
        for name in ["node", "python", "uv", "docker"] {
            assert!(
                !hints(name).is_empty(),
                "{name} has nowhere to send anybody"
            );
        }
    }
}
