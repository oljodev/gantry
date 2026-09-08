//! Listing, finding by name and finding by content (`docs/connectors/filesystem.md` §5, §7).
//!
//! All three walk the tree the same way, through `ignore`, so the project's own ignore rules and
//! the small floor of never-interesting directories are applied once and identically (D4). A
//! search in a JavaScript project returns the user's code rather than ten thousand matches from
//! its dependencies — and a file left out of the results can still be read by name, which is the
//! other half of D4 and lives in `read_file`.
//!
//! Nothing here opens a file that a caller then writes: enumeration is rooted at a folder that
//! was resolved through the root handle and never follows a link out of it, and anything acted
//! on afterwards is re-resolved from the root (D12).

use std::path::Path;

use ignore::WalkBuilder;
use serde::Serialize;

/// Caps, each reported rather than silently applied (D9).
const MAX_ENTRIES: usize = 500;
const MAX_PATHS: usize = 500;
const MAX_MATCHES: usize = 200;
/// Bytes of a file past which content search does not read it. Source files are far below this;
/// a minified bundle or a checked-in database is far above, and searching it helps nobody.
const MAX_SEARCHED_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub name: String,
    pub path: String,
    /// `file`, `dir` or `symlink`.
    pub kind: &'static str,
    pub size: Option<u64>,
    pub modified: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Match {
    pub path: String,
    pub line: u32,
    pub text: String,
}

/// What a search returns, and what it had to leave out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Found<T> {
    pub items: Vec<T>,
    /// How many more there were past the cap; zero when everything fitted.
    pub more: usize,
}

impl<T> Default for Found<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            more: 0,
        }
    }
}

/// How much of a content search comes back: which files, the lines themselves, or a count per
/// file. Three modes rather than one, because "which files mention this" and "show me the lines"
/// have very different token costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrepMode {
    Files,
    Lines,
    Count,
}

/// One level of a folder. Hidden files and ignored files are left out unless `all` is set;
/// either way they remain readable by name.
#[must_use]
pub fn list(dir: &Path, all: bool) -> Found<Entry> {
    let mut found = Found::default();
    for entry in walker(dir, all, false).max_depth(Some(1)).build().flatten() {
        if entry.depth() == 0 {
            continue;
        }
        if found.items.len() == MAX_ENTRIES {
            found.more += 1;
            continue;
        }
        found.items.push(describe(entry.path(), entry.file_type()));
    }
    found.items.sort_by(|a, b| {
        (a.kind == "file")
            .cmp(&(b.kind == "file"))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    found
}

/// Paths matching a glob, most recently modified first, which is almost always the order a
/// person or a model wants: the file being worked on is the one that changed last.
pub fn glob(root: &Path, pattern: &str, all: bool) -> Result<Found<String>, globset::Error> {
    let matcher = globset::GlobBuilder::new(pattern)
        .literal_separator(!pattern.contains("**"))
        .build()?
        .compile_matcher();
    let mut hits: Vec<(Option<i64>, String)> = Vec::new();
    let mut more = 0;
    for entry in walker(root, all, true).build().flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = entry.path();
        let relative = path.strip_prefix(root).unwrap_or(path);
        if !(matcher.is_match(relative) || matcher.is_match(path)) {
            continue;
        }
        if hits.len() == MAX_PATHS {
            more += 1;
            continue;
        }
        hits.push((modified(path), path.display().to_string()));
    }
    hits.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(Found {
        items: hits.into_iter().map(|(_, p)| p).collect(),
        more,
    })
}

/// Lines matching a regular expression. `filter` narrows the files searched by the same glob
/// syntax `glob` takes.
pub fn grep(
    root: &Path,
    pattern: &str,
    filter: Option<&str>,
    case_sensitive: bool,
    all: bool,
) -> Result<Found<Match>, String> {
    let regex = regex::RegexBuilder::new(pattern)
        .case_insensitive(!case_sensitive)
        .build()
        .map_err(|err| format!("that is not a valid regular expression: {err}"))?;
    let filter = match filter {
        Some(glob) => Some(
            globset::GlobBuilder::new(glob)
                .literal_separator(!glob.contains("**"))
                .build()
                .map_err(|err| format!("that is not a valid file pattern: {err}"))?
                .compile_matcher(),
        ),
        None => None,
    };

    let mut found: Found<Match> = Found::default();
    for entry in walker(root, all, true).build().flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = entry.path();
        if let Some(filter) = &filter {
            let relative = path.strip_prefix(root).unwrap_or(path);
            if !(filter.is_match(relative) || filter.is_match(path)) {
                continue;
            }
        }
        if entry.metadata().is_ok_and(|m| m.len() > MAX_SEARCHED_BYTES) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            continue; // Binary, or unreadable: not an error, just not a match.
        };
        for (n, line) in text.lines().enumerate() {
            if !regex.is_match(line) {
                continue;
            }
            if found.items.len() == MAX_MATCHES {
                found.more += 1;
                continue;
            }
            found.items.push(Match {
                path: path.display().to_string(),
                line: n as u32 + 1,
                // A very long line would otherwise cost more than the answer is worth.
                text: line.chars().take(400).collect(),
            });
        }
    }
    Ok(found)
}

/// The matches of `grep` collapsed to one row per file, for the two cheaper modes.
#[must_use]
pub fn by_file(found: &Found<Match>) -> Vec<(String, usize)> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for m in &found.items {
        match counts.iter_mut().find(|(p, _)| *p == m.path) {
            Some((_, n)) => *n += 1,
            None => counts.push((m.path.clone(), 1)),
        }
    }
    counts
}

/// The walker every one of them uses. `follow_links(false)` matters: a link out of the folder
/// must not become a way to enumerate what is on the other side of the boundary.
///
/// `floor` is the set of directories that are never interesting to *search* (§7's wording).
/// Searching applies it; listing does not, because being told a folder is not there when the
/// user can see that it is, is worse than the noise D4 was written to avoid.
fn walker(root: &Path, all: bool, floor: bool) -> WalkBuilder {
    let mut builder = WalkBuilder::new(root);
    builder
        .follow_links(false)
        .hidden(!all)
        .git_ignore(!all)
        .git_exclude(!all)
        .ignore(!all)
        // Only ignore files *inside* the attached folder count. `parents` would read ignore
        // files above it and `git_global` the user's own, so what is visible inside the
        // boundary would be decided by files outside it — surprising to explain, impossible to
        // predict from the folder itself, and a small leak of what is out there.
        .parents(false)
        .git_global(false)
        // A folder the user attached is not necessarily a git repository, and its `.gitignore`
        // means what it says either way. Without this, ignore rules apply only inside a
        // checkout, which is exactly where a user is least likely to notice they did not.
        .require_git(false)
        .max_filesize(None);
    if !all && floor {
        // Directories no project wants searched, whether or not it has an ignore file. `ignore`
        // handles the rest from the folder's own rules.
        let mut overrides = ignore::overrides::OverrideBuilder::new(root);
        for never in ["!**/node_modules/**", "!**/target/**", "!**/.git/**"] {
            let _ = overrides.add(never);
        }
        if let Ok(overrides) = overrides.build() {
            builder.overrides(overrides);
        }
    }
    builder
}

fn describe(path: &Path, file_type: Option<std::fs::FileType>) -> Entry {
    let meta = std::fs::symlink_metadata(path).ok();
    Entry {
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        path: path.display().to_string(),
        kind: match file_type {
            Some(t) if t.is_dir() => "dir",
            Some(t) if t.is_symlink() => "symlink",
            _ => "file",
        },
        size: meta.as_ref().filter(|m| m.is_file()).map(|m| m.len()),
        modified: meta.as_ref().and_then(millis),
    }
}

fn modified(path: &Path) -> Option<i64> {
    std::fs::metadata(path).ok().as_ref().and_then(millis)
}

fn millis(meta: &std::fs::Metadata) -> Option<i64> {
    meta.modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("node_modules/left-pad")).unwrap();
        std::fs::write(root.join(".gitignore"), "build/\n").unwrap();
        std::fs::create_dir_all(root.join("build")).unwrap();
        std::fs::write(root.join("build/out.js"), "needle in the build\n").unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {\n    // needle\n}\n").unwrap();
        std::fs::write(root.join("src/util.rs"), "pub fn helper() {}\n").unwrap();
        std::fs::write(root.join("node_modules/left-pad/index.js"), "needle\n").unwrap();
        std::fs::write(root.join(".env"), "TOKEN=abc\n").unwrap();
        std::fs::write(root.join("README.md"), "# Title\n").unwrap();
        dir
    }

    #[test]
    fn listing_hides_dotfiles_and_ignored_folders_until_asked() {
        let dir = tree();
        let found = list(dir.path(), false);
        let names: Vec<String> = found.items.iter().map(|e| e.name.clone()).collect();
        let has = |n: &str| names.iter().any(|name| name == n);
        assert!(has("src"), "{names:?}");
        assert!(has("README.md"), "{names:?}");
        assert!(!has(".env"), "{names:?}");
        assert!(
            !has("build"),
            "the folder's own ignore rules apply: {names:?}"
        );
        assert!(
            has("node_modules"),
            "a listing shows what the user can see; the search floor is not a listing filter"
        );

        let all = list(dir.path(), true);
        let names: Vec<String> = all.items.iter().map(|e| e.name.clone()).collect();
        assert!(names.contains(&".env".to_owned()), "{names:?}");
        assert!(names.contains(&"build".to_owned()), "{names:?}");
        // Folders sort before files, so the first entry is never a file when both are present.
        assert_eq!(all.items[0].kind, "dir");
    }

    #[test]
    fn a_glob_finds_source_and_not_dependencies() {
        let dir = tree();
        let found = glob(dir.path(), "**/*.rs", false).unwrap();
        assert_eq!(found.items.len(), 2, "{found:?}");
        assert!(found.items.iter().all(|p| p.ends_with(".rs")));
        let js = glob(dir.path(), "**/*.js", false).unwrap();
        assert!(js.items.is_empty(), "{js:?}");
    }

    #[test]
    fn grep_searches_the_users_code_and_reports_where() {
        let dir = tree();
        let found = grep(dir.path(), "needle", None, false, false).unwrap();
        assert_eq!(found.items.len(), 1, "{found:?}");
        assert_eq!(found.items[0].line, 2);
        assert!(found.items[0].path.ends_with("main.rs"));
        assert_eq!(by_file(&found), vec![(found.items[0].path.clone(), 1)]);

        // Case folds by default, and the filter narrows to a subset.
        assert_eq!(
            grep(dir.path(), "NEEDLE", None, false, false)
                .unwrap()
                .items
                .len(),
            1
        );
        assert!(
            grep(dir.path(), "needle", Some("**/*.md"), false, false)
                .unwrap()
                .items
                .is_empty()
        );
        assert!(
            grep(dir.path(), "needle", None, true, false)
                .unwrap()
                .items
                .len()
                == 1
        );
        assert!(grep(dir.path(), "(", None, false, false).is_err());
    }

    #[test]
    fn searching_everything_reaches_what_the_ignore_rules_hid() {
        let dir = tree();
        let found = grep(dir.path(), "needle", None, false, true).unwrap();
        assert_eq!(found.items.len(), 3, "{found:?}");
    }
}
