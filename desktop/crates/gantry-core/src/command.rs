//! The command classifier (`docs/connectors/shell.md` §6, `docs/plan/03-connector-system.md` §5).
//!
//! It answers one question: can this command line be *proved* to only observe? A command that
//! can is treated as a `read` tool for permission purposes, which is what lets Plan mode allow
//! `git status` while refusing everything else, and what stops Auto-edit asking about `ls`.
//!
//! It lives in `gantry-core` rather than in the shell connector because two unrelated places
//! need the same answer: the connector, which reports it in the result and in its refusals, and
//! the permission engine, which cannot depend on a connector crate.
//!
//! **What a verdict is worth.** [`ReadOnly`](CommandClass::ReadOnly) is a property of the
//! *string*, not of the execution: a shell can resolve a name to something other than the
//! program anyone expects. The connector closes that gap for the ordinary case by running
//! commands in a shell that does not read the user's startup files (shell.md D2), and it stays
//! open for an allowlisted program that has been replaced on disk. The interface says
//! "Gantry checked that this command only reads", never "this command cannot change anything".

use std::collections::HashSet;

/// What the classifier could prove about a command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandClass {
    /// Every segment runs a program that only observes.
    ReadOnly,
    /// Something here could change state. The reason is shown to the user and to the model.
    Effectful(String),
}

impl CommandClass {
    #[must_use]
    pub fn is_read_only(&self) -> bool {
        matches!(self, Self::ReadOnly)
    }

    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::ReadOnly => None,
            Self::Effectful(reason) => Some(reason),
        }
    }
}

/// Programs that observe and change nothing, whatever their arguments.
const OBSERVERS: &[&str] = &[
    "ls",
    "cat",
    "head",
    "tail",
    "wc",
    "stat",
    "file",
    "tree",
    "du",
    "df",
    "pwd",
    "echo",
    "which",
    "type",
    "whoami",
    "hostname",
    "uname",
    "date",
    "ps",
    "env",
    "printenv",
    "basename",
    "dirname",
    "realpath",
    "readlink",
    "grep",
    "egrep",
    "fgrep",
    "rg",
    "ag",
    "diff",
    "cmp",
    "sort",
    "uniq",
    "cut",
    "column",
    "nl",
    "sha256sum",
    "md5sum",
    "true",
    "false",
    "sleep",
    "id",
    "groups",
    "locale",
    "arch",
    "nproc",
    "uptime",
    "free",
    "lscpu",
    "jq",
    "yq",
];

/// Programs whose read-only subcommands are listed, and whose others are not.
const SUBCOMMANDS: &[(&str, &[&str])] = &[
    (
        "git",
        &[
            "status",
            "log",
            "diff",
            "show",
            "branch",
            "blame",
            "rev-parse",
            "ls-files",
            "ls-remote",
            "describe",
            "shortlog",
            "config",
            "remote",
            "tag",
            "stash",
            "worktree",
        ],
    ),
    ("cargo", &["metadata", "tree", "search", "--version"]),
    ("npm", &["ls", "list", "view", "outdated", "why"]),
    ("pnpm", &["ls", "list", "why", "outdated"]),
    ("yarn", &["list", "why", "info"]),
    ("docker", &["ps", "images", "logs", "inspect", "version"]),
    ("kubectl", &["get", "describe", "logs", "version"]),
];

/// Subcommands of the above that read despite the parent's own subcommand being read-only:
/// `git config --get` observes, `git config --unset` does not; `git remote -v` observes,
/// `git remote add` does not; `git stash list` observes, `git stash pop` does not.
const SUBCOMMAND_MUST_NOT_HAVE: &[(&str, &str, &[&str])] = &[
    (
        "git",
        "config",
        &["--unset", "--add", "--replace-all", "--edit", "-e"],
    ),
    (
        "git",
        "remote",
        &["add", "remove", "rm", "rename", "set-url", "prune"],
    ),
    ("git", "tag", &["-d", "--delete", "-a", "-f", "--force"]),
    (
        "git",
        "stash",
        &["pop", "apply", "drop", "clear", "push", "save"],
    ),
    ("git", "worktree", &["add", "remove", "prune", "move"]),
    (
        "git",
        "branch",
        &[
            "-d", "-D", "--delete", "-m", "-M", "--move", "-f", "--force",
        ],
    ),
    ("docker", "logs", &["--follow", "-f"]),
    ("kubectl", "logs", &["--follow", "-f"]),
];

/// Programs whose only allowed use is being asked what version they are. `node --version` is a
/// question; `node script.js` is arbitrary code, and the two differ only in the arguments.
const VERSION_ONLY: &[&str] = &[
    "node",
    "python",
    "python3",
    "ruby",
    "go",
    "java",
    "javac",
    "rustc",
    "deno",
    "bun",
    "php",
    "dotnet",
    "swift",
    "gcc",
    "clang",
    "make",
    "cmake",
    "terraform",
    "aws",
    "gh",
    "uv",
    "pip",
    "pip3",
    "poetry",
    "ruff",
    "eslint",
    "tsc",
    "psql",
    "sqlite3",
];

const VERSION_FLAGS: &[&str] = &["--version", "-V", "-v", "--help", "-h", "version"];

/// Programs that run *another* program. Allowlisting one of these would let anything through
/// behind it, so they are unwrapped and whatever they were going to run is classified instead.
/// This is the `env` / `nice` / `xargs` hole, closed rather than noted.
const WRAPPERS: &[&str] = &[
    "env", "nice", "ionice", "nohup", "time", "timeout", "stdbuf", "command", "xargs", "watch",
    "sudo", "doas", "su", "setsid", "chroot", "unbuffer", "script",
];

/// Wrappers that are refused outright rather than unwrapped: what they run is the user's
/// privileges, not the user's program.
const NEVER: &[&str] = &["sudo", "doas", "su", "chroot", "setsid"];

/// `find` arguments that make it act rather than look.
const FIND_ACTIONS: &[&str] = &[
    "-delete", "-exec", "-execdir", "-ok", "-okdir", "-fls", "-fprint", "-fprintf",
];

/// PowerShell's read-only surface: the `Get-` verb, plus the three aliases people actually type.
fn powershell_read_only(program: &str) -> bool {
    let lower = program.to_ascii_lowercase();
    lower.starts_with("get-")
        || lower.starts_with("measure-")
        || lower.starts_with("test-")
        || matches!(
            lower.as_str(),
            "dir" | "gci" | "select-string" | "where-object"
        )
}

/// Classify one command line.
#[must_use]
pub fn classify(command: &str) -> CommandClass {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return CommandClass::Effectful("the command is empty".to_owned());
    }
    if let Some(reason) = dangerous_syntax(trimmed) {
        return CommandClass::Effectful(reason);
    }
    let segments = split_segments(trimmed);
    if segments.is_empty() {
        return CommandClass::Effectful("the command is empty".to_owned());
    }
    for segment in segments {
        if let CommandClass::Effectful(reason) = classify_segment(&segment) {
            return CommandClass::Effectful(reason);
        }
    }
    CommandClass::ReadOnly
}

/// Syntax that can write a file or run something the tokens never name. Checked on the whole
/// line, outside quotes, because a redirection in any segment writes just as well as in the first.
fn dangerous_syntax(command: &str) -> Option<String> {
    let bytes: Vec<char> = command.chars().collect();
    let mut single = false;
    let mut double = false;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        let escaped = i > 0 && bytes[i - 1] == '\\';
        match c {
            '\'' if !double && !escaped => single = !single,
            '"' if !single && !escaped => double = !double,
            '`' if !single && !escaped => {
                return Some("backticks can run any command".to_owned());
            }
            '$' if !single && !escaped && bytes.get(i + 1) == Some(&'(') => {
                return Some("command substitution can run any command".to_owned());
            }
            '>' if !single && !double && !escaped => {
                return Some("a redirection writes a file".to_owned());
            }
            '<' if !single && !double && !escaped => {
                // `<` reads, but `<<<` and process substitution `<(` do more, and a here-doc
                // feeds input a closed stdin cannot supply anyway.
                return Some("a redirection is not read-only".to_owned());
            }
            '&' if !single && !double && !escaped => {
                let next = bytes.get(i + 1);
                if next != Some(&'&') && !(i > 0 && bytes[i - 1] == '&') {
                    return Some("`&` puts the command in the background".to_owned());
                }
            }
            _ => {}
        }
        i += 1;
    }
    if single || double {
        return Some("the quotes in this command are unbalanced".to_owned());
    }
    None
}

/// Split on the operators that chain commands, respecting quotes.
fn split_segments(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut single = false;
    let mut double = false;
    let chars: Vec<char> = command.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let escaped = i > 0 && chars[i - 1] == '\\';
        if c == '\'' && !double && !escaped {
            single = !single;
        } else if c == '"' && !single && !escaped {
            double = !double;
        }
        if !single && !double && !escaped {
            let two: String = chars[i..(i + 2).min(chars.len())].iter().collect();
            if two == "&&" || two == "||" {
                out.push(std::mem::take(&mut current));
                i += 2;
                continue;
            }
            if c == ';' || c == '|' || c == '\n' {
                out.push(std::mem::take(&mut current));
                i += 1;
                continue;
            }
        }
        current.push(c);
        i += 1;
    }
    out.push(current);
    out.into_iter()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect()
}

fn classify_segment(segment: &str) -> CommandClass {
    let Ok(tokens) = shell_words::split(segment) else {
        return CommandClass::Effectful("this command could not be parsed".to_owned());
    };
    let mut rest: &[String] = &tokens;
    // Peel `VAR=value` prefixes and wrapper programs until a real program is in front.
    loop {
        let Some(first) = rest.first() else {
            return CommandClass::Effectful("this command has no program to check".to_owned());
        };
        if first.contains('=') && !first.starts_with('-') {
            rest = &rest[1..];
            continue;
        }
        let program = basename(first);
        if NEVER.contains(&program.as_str()) {
            return CommandClass::Effectful(format!("`{program}` runs as another user"));
        }
        if WRAPPERS.contains(&program.as_str()) && rest.len() > 1 {
            // Skip the wrapper's own options and classify what it was going to run. `xargs cat`
            // runs `cat`; `env FOO=1 rm` runs `rm`, whatever `env`'s own listing would say.
            let mut i = 1;
            while i < rest.len() && is_option_or_value(&rest[i]) {
                i += 1;
            }
            if i >= rest.len() {
                // `env` with no program is the observer that prints the environment.
                return if program == "env" || program == "printenv" {
                    CommandClass::ReadOnly
                } else {
                    CommandClass::Effectful(format!("`{program}` runs another program"))
                };
            }
            rest = &rest[i..];
            continue;
        }
        return classify_program(&program, first, &rest[1..]);
    }
}

fn classify_program(program: &str, raw: &str, args: &[String]) -> CommandClass {
    if raw.contains('/') || raw.contains('\\') {
        return CommandClass::Effectful(format!(
            "`{raw}` runs a program by path, which cannot be checked"
        ));
    }
    if program == "find" {
        if let Some(action) = args.iter().find(|a| FIND_ACTIONS.contains(&a.as_str())) {
            return CommandClass::Effectful(format!("`find {action}` acts on what it finds"));
        }
        return CommandClass::ReadOnly;
    }
    if OBSERVERS.contains(&program) {
        return CommandClass::ReadOnly;
    }
    if let Some((_, allowed)) = SUBCOMMANDS.iter().find(|(name, _)| *name == program) {
        let Some(sub) = args.iter().find(|a| !a.starts_with('-')) else {
            // `git` alone prints its usage; so does every other tool here.
            return CommandClass::ReadOnly;
        };
        if !allowed.contains(&sub.as_str()) {
            return CommandClass::Effectful(format!(
                "`{program} {sub}` is not a read-only command"
            ));
        }
        let forbidden: HashSet<&str> = SUBCOMMAND_MUST_NOT_HAVE
            .iter()
            .filter(|(p, s, _)| *p == program && *s == sub.as_str())
            .flat_map(|(_, _, flags)| flags.iter().copied())
            .collect();
        if let Some(bad) = args.iter().find(|a| forbidden.contains(a.as_str())) {
            return CommandClass::Effectful(format!("`{program} {sub} {bad}` changes something"));
        }
        return CommandClass::ReadOnly;
    }
    if VERSION_ONLY.contains(&program) {
        return if args.len() == 1 && VERSION_FLAGS.contains(&args[0].as_str()) {
            CommandClass::ReadOnly
        } else {
            CommandClass::Effectful(format!(
                "`{program}` runs code unless it is only asked its version"
            ))
        };
    }
    if powershell_read_only(program) {
        return CommandClass::ReadOnly;
    }
    CommandClass::Effectful(format!("`{program}` is not on the read-only list"))
}

/// A wrapper's own option, its value, or a variable assignment — anything that is not yet the
/// program being wrapped. `nice -n 10 git status` runs `git`, not `10`.
fn is_option_or_value(token: &str) -> bool {
    token.starts_with('-')
        || token.contains('=')
        || token
            .trim_end_matches(['s', 'm', 'h', 'd'])
            .parse::<f64>()
            .is_ok()
}

fn basename(program: &str) -> String {
    program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program)
        .trim_end_matches(".exe")
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reads(command: &str) -> bool {
        classify(command).is_read_only()
    }

    #[test]
    fn the_ordinary_read_only_commands_pass() {
        for command in [
            "ls",
            "ls -la src",
            "cat Cargo.toml",
            "git status",
            "git log --oneline -20",
            "git diff HEAD~1",
            "rg 'fn main' --type rust",
            "find . -name '*.rs'",
            "cargo tree",
            "npm ls --depth 0",
            "node --version",
            "wc -l src/lib.rs | head",
            "git status && git diff",
            "printenv",
            "env",
        ] {
            assert!(reads(command), "should read: {command}");
        }
    }

    #[test]
    fn anything_that_changes_something_does_not() {
        for command in [
            "rm -rf build",
            "cargo build",
            "npm install",
            "git commit -m 'x'",
            "git push",
            "node server.js",
            "python -c 'import os; os.remove(\"x\")'",
            "./configure",
            "/bin/rm x",
            "make",
        ] {
            assert!(!reads(command), "should not read: {command}");
        }
    }

    #[test]
    fn a_wrapper_does_not_launder_the_program_it_runs() {
        // The whole point: `env` and `nice` and `xargs` are on nobody's allowlist as a way in.
        assert!(!reads("env rm -rf /"));
        assert!(!reads("env FOO=1 rm -rf build"));
        assert!(!reads("nice -n 10 cargo build"));
        assert!(!reads("xargs rm"));
        assert!(!reads("timeout 5 npm install"));
        assert!(!reads("nohup node server.js"));
        // …and unwrapping keeps working for the harmless case.
        assert!(reads("env ls"));
        assert!(reads("nice -n 10 git status"));
        assert!(reads("xargs cat"));
        assert!(reads("timeout 5 ls"));
    }

    #[test]
    fn privilege_escalation_is_refused_whatever_follows() {
        assert!(!reads("sudo ls"));
        assert!(!reads("sudo -u root cat /etc/shadow"));
        assert!(!reads("doas ls"));
        assert!(!reads("su - olav -c ls"));
    }

    #[test]
    fn substitution_and_redirection_are_refused_anywhere_in_the_line() {
        assert!(!reads("echo $(rm -rf build)"));
        assert!(!reads("echo `rm -rf build`"));
        assert!(!reads("ls > listing.txt"));
        assert!(!reads("ls >> listing.txt"));
        assert!(!reads("cat < input.txt"));
        assert!(!reads("git status; echo $(whoami)"));
        assert!(!reads("ls &"));
        // A `$` that is not substitution is fine, and so is `&&`.
        assert!(reads("echo $HOME"));
        assert!(reads("ls && git status"));
    }

    #[test]
    fn every_segment_has_to_pass() {
        assert!(!reads("git status && rm -rf build"));
        assert!(!reads("ls; cargo build"));
        assert!(!reads("cat file | tee copy.txt"));
        assert!(reads("cat file | grep foo | wc -l"));
    }

    #[test]
    fn an_allowlisted_program_with_dangerous_arguments_does_not_pass() {
        assert!(!reads("find . -name '*.tmp' -delete"));
        assert!(!reads("find . -exec rm {} ;"));
        assert!(!reads("git config --unset user.email"));
        assert!(!reads("git remote add origin git@example.com:x/y.git"));
        assert!(!reads("git branch -D main"));
        assert!(!reads("git stash pop"));
        assert!(reads("git config --get user.email"));
        assert!(reads("git remote -v"));
        assert!(reads("git stash list"));
    }

    #[test]
    fn a_program_named_by_path_is_never_proven() {
        assert!(!reads("./ls"));
        assert!(!reads("/usr/bin/ls"));
        assert!(!reads("../bin/git status"));
    }

    #[test]
    fn unparseable_and_empty_commands_are_effectful() {
        assert!(!reads(""));
        assert!(!reads("   "));
        assert!(!reads("echo 'unbalanced"));
    }

    #[test]
    fn the_reason_names_what_was_wrong() {
        let verdict = classify("git status && rm -rf build");
        assert_eq!(verdict.reason(), Some("`rm` is not on the read-only list"));
        let verdict = classify("ls > out.txt");
        assert_eq!(verdict.reason(), Some("a redirection writes a file"));
    }

    #[test]
    fn powershell_reads_are_recognised() {
        assert!(reads("Get-ChildItem"));
        assert!(reads("Select-String -Pattern foo"));
        assert!(!reads("Remove-Item -Recurse build"));
    }
}
