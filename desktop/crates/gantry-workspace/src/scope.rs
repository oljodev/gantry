//! Containment: which files a chat may touch (`docs/connectors/filesystem.md` §4).
//!
//! The rule that shapes everything here is that containment is **not** decided by comparing
//! path strings. String work chooses which root a path claims and rejects hostile syntax; the
//! open itself goes through a directory handle that cannot escape, so the kernel — which knows
//! where a path leads, symlinks and case folding included — is the one that answers. Every
//! failure is a refusal (D2, fail closed), and nothing is ever cached as already-validated
//! (D12): a model can plant a link in one turn and use it three turns later.

use std::path::{Component, Path, PathBuf};

use cap_std::{ambient_authority, fs::Dir};

/// Longest path Gantry will look at, and the deepest it will walk. Both are far past anything
/// a real project needs, and both keep a hostile argument from becoming a stack or memory
/// problem before it is refused.
const MAX_PATH_LEN: usize = 4096;
const MAX_DEPTH: usize = 64;

#[derive(Debug, thiserror::Error)]
pub enum ScopeError {
    #[error("{0}")]
    Syntax(String),
    /// No attached folder contains the path. Carries what the model needs to ask for one (D5).
    #[error("{path} is not inside any folder attached to this chat")]
    Outside { path: String, roots: Vec<String> },
    #[error("{0} is inside more than one attached folder; name a path under just one of them")]
    Ambiguous(String),
    /// Gantry's own configuration, whatever the roots say (D6).
    #[error("{0} belongs to Gantry itself and is never writable")]
    Denied(String),
    #[error("{0} does not exist")]
    NotFound(String),
    #[error("{0}")]
    Io(String),
}

/// One folder the user attached, held open for as long as the roots live so that the folder
/// cannot be renamed or replaced underneath us.
pub struct Root {
    /// The canonical path, which is what is reported back to the user and the model.
    pub path: PathBuf,
    dir: Dir,
}

/// The folders one chat may reach, plus the paths nothing may ever write.
pub struct Roots {
    roots: Vec<Root>,
    denied: Vec<PathBuf>,
}

/// A path that has been through every phase: it resolved into exactly one root, and it is
/// opened relative to that root's handle rather than by name.
#[derive(Debug)]
pub struct Scoped<'a> {
    dir: &'a Dir,
    /// Relative to the root, and never rejoined into an absolute string for opening.
    rel: PathBuf,
    /// The absolute path, for messages, the journal and the feed.
    pub path: PathBuf,
}

impl Roots {
    /// Opens each folder the user attached. A folder that cannot be opened is left out with a
    /// warning rather than taking the chat down: the others still work, and a path into the
    /// missing one is refused by the ordinary out-of-scope route.
    ///
    /// `denied` is Gantry's own data: canonicalized here, compared by components later.
    pub fn open(paths: &[String], denied: &[PathBuf]) -> Self {
        let mut roots = Vec::new();
        for path in paths {
            match Dir::open_ambient_dir(path, ambient_authority()) {
                Ok(dir) => {
                    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.into());
                    roots.push(Root {
                        path: canonical,
                        dir,
                    });
                }
                Err(err) => log::warn!("the attached folder {path} cannot be opened: {err}"),
            }
        }
        Self {
            roots,
            denied: denied
                .iter()
                .map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.clone()))
                .collect(),
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    #[must_use]
    pub fn paths(&self) -> Vec<String> {
        self.roots
            .iter()
            .map(|r| r.path.display().to_string())
            .collect()
    }

    /// The five phases of `filesystem.md` §4, in order. Phases 0 to 2 are string work and are
    /// allowed to refuse; phase 3, the capability open, is the boundary; phase 4 is the deny
    /// list, applied last.
    pub fn resolve(&self, path: &str) -> Result<Scoped<'_>, ScopeError> {
        let raw = syntax_gate(path)?;
        let (root, rel) = self.anchor(&raw)?;
        if rel.components().count() > MAX_DEPTH {
            return Err(ScopeError::Syntax(format!(
                "{path} is deeper than {MAX_DEPTH} components"
            )));
        }
        let absolute = root.path.join(&rel);
        if self
            .denied
            .iter()
            .any(|d| starts_with_components(&absolute, d))
        {
            return Err(ScopeError::Denied(absolute.display().to_string()));
        }
        Ok(Scoped {
            dir: &root.dir,
            rel,
            path: absolute,
        })
    }

    /// Phase 1: an absolute path must component-prefix-match exactly one root. Zero is a
    /// request to add a folder; more than one is an error rather than a guess.
    fn anchor(&self, path: &Path) -> Result<(&Root, PathBuf), ScopeError> {
        let matches: Vec<&Root> = self
            .roots
            .iter()
            .filter(|r| starts_with_components(path, &r.path))
            .collect();
        match matches.len() {
            0 => Err(ScopeError::Outside {
                path: path.display().to_string(),
                roots: self.paths(),
            }),
            1 => {
                let root = matches[0];
                let rel = path.strip_prefix(&root.path).unwrap_or(path).to_path_buf();
                Ok((root, rel))
            }
            _ => Err(ScopeError::Ambiguous(path.display().to_string())),
        }
    }
}

impl Scoped<'_> {
    /// Phase 3 for reading: the open goes through the root's handle with the relative
    /// components only, so a link that leaves the folder fails here rather than being caught
    /// by a comparison afterwards.
    pub fn read(&self) -> Result<Vec<u8>, ScopeError> {
        self.dir.read(&self.rel).map_err(|err| self.io(&err))
    }

    pub fn exists(&self) -> bool {
        self.dir.metadata(&self.rel).is_ok()
    }

    /// The atomic write of `filesystem.md` §5: a temporary file beside the target, the
    /// original's permissions carried over, then a rename. A reader either sees the old file
    /// or the new one, never a half-written one, and a crash loses the temporary file rather
    /// than the user's work.
    pub fn write_atomically(&self, bytes: &[u8]) -> Result<(), ScopeError> {
        use cap_std::fs::OpenOptions;
        use std::io::Write;

        let parent = self.rel.parent().unwrap_or(Path::new(""));
        let name = self
            .rel
            .file_name()
            .ok_or_else(|| ScopeError::Syntax(format!("{} names no file", self.path.display())))?;
        let temp = parent.join(format!(
            ".{}.gantry-{}",
            name.to_string_lossy(),
            std::process::id()
        ));
        let permissions = self.dir.metadata(&self.rel).ok().map(|m| m.permissions());

        let mut file = self
            .dir
            .open_with(&temp, OpenOptions::new().write(true).create_new(true))
            .map_err(|err| self.io(&err))?;
        let written = file
            .write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|err| self.io(&err));
        if let Err(err) = written {
            let _ = self.dir.remove_file(&temp);
            return Err(err);
        }
        if let Some(permissions) = permissions
            && let Err(err) = file.set_permissions(permissions)
        {
            log::warn!(
                "{}: keeping the default permissions: {err}",
                self.path.display()
            );
        }
        drop(file);

        if let Err(err) = self.dir.rename(&temp, self.dir, &self.rel) {
            let _ = self.dir.remove_file(&temp);
            return Err(self.io(&err));
        }
        Ok(())
    }

    fn io(&self, err: &std::io::Error) -> ScopeError {
        if err.kind() == std::io::ErrorKind::NotFound {
            ScopeError::NotFound(self.path.display().to_string())
        } else {
            ScopeError::Io(format!("{}: {err}", self.path.display()))
        }
    }
}

/// Phase 0 and phase 2: reject, never sanitise. What survives is an absolute path with no `..`
/// component, no null byte, and nothing in the Windows family of alternate spellings.
fn syntax_gate(path: &str) -> Result<PathBuf, ScopeError> {
    if path.trim().is_empty() {
        return Err(ScopeError::Syntax("the path is empty".to_owned()));
    }
    if path.contains('\0') {
        return Err(ScopeError::Syntax(
            "the path contains a null byte".to_owned(),
        ));
    }
    if path.len() > MAX_PATH_LEN {
        return Err(ScopeError::Syntax(format!(
            "the path is longer than {MAX_PATH_LEN} characters"
        )));
    }
    #[cfg(windows)]
    windows_gate(path)?;

    let path = PathBuf::from(path);
    if !path.is_absolute() {
        return Err(ScopeError::Syntax(format!(
            "{} is not an absolute path; paths are absolute, as the user sees them",
            path.display()
        )));
    }
    // Phase 2: `..` is refused rather than resolved, because an intervening component may be a
    // symlink and resolving it lexically would change which file the path names.
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(ScopeError::Syntax(format!(
            "{} contains `..`; name the path from the folder instead",
            path.display()
        )));
    }
    Ok(path)
}

/// The Windows spellings that open a different file than they appear to: alternate data
/// streams, reserved device names, components that end in a dot or a space, and the verbatim,
/// device and network prefixes.
#[cfg(windows)]
fn windows_gate(path: &str) -> Result<(), ScopeError> {
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if path.starts_with(r"\\") || path.starts_with("//") {
        return Err(ScopeError::Syntax(
            "verbatim, device and network paths are refused".to_owned(),
        ));
    }
    let after_drive = match path.as_bytes() {
        [drive, b':', rest @ ..] if drive.is_ascii_alphabetic() => {
            if !matches!(rest.first(), Some(b'\\' | b'/')) {
                return Err(ScopeError::Syntax(
                    "a drive-relative path is refused; name the full path".to_owned(),
                ));
            }
            &path[2..]
        }
        _ => path,
    };
    if after_drive.contains(':') {
        return Err(ScopeError::Syntax(
            "a colon outside the drive letter names an alternate data stream".to_owned(),
        ));
    }
    for part in after_drive.split(['\\', '/']).filter(|p| !p.is_empty()) {
        if part.ends_with('.') || part.ends_with(' ') {
            return Err(ScopeError::Syntax(format!(
                "the path component `{part}` ends in a dot or a space, which Windows strips"
            )));
        }
        let stem = part.split('.').next().unwrap_or(part).to_ascii_uppercase();
        if RESERVED.contains(&stem.as_str()) {
            return Err(ScopeError::Syntax(format!(
                "`{part}` is a reserved device name"
            )));
        }
    }
    Ok(())
}

/// Component-by-component prefix, never a string prefix: `…/project` must not match
/// `…/project-evil`. Case folding follows the platform, which is the pragmatic reading of a
/// per-volume question.
#[must_use]
pub fn starts_with_components(path: &Path, prefix: &Path) -> bool {
    let mut want = prefix.components();
    let mut have = path.components();
    loop {
        match (want.next(), have.next()) {
            (None, _) => return true,
            (Some(_), None) => return false,
            (Some(a), Some(b)) if same_component(&a, &b) => {}
            _ => return false,
        }
    }
}

fn same_component(a: &Component<'_>, b: &Component<'_>) -> bool {
    if cfg!(any(windows, target_os = "macos")) {
        a.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(&b.as_os_str().to_string_lossy())
    } else {
        a == b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots(dir: &Path) -> Roots {
        Roots::open(&[dir.display().to_string()], &[])
    }

    #[test]
    fn a_sibling_folder_with_the_same_prefix_is_not_inside_the_root() {
        assert!(starts_with_components(
            Path::new("/home/a/project/src/main.rs"),
            Path::new("/home/a/project")
        ));
        assert!(!starts_with_components(
            Path::new("/home/a/project-evil/x"),
            Path::new("/home/a/project")
        ));
    }

    #[test]
    fn the_syntax_gate_refuses_what_it_cannot_trust() {
        for bad in ["", "relative/path.rs", "/a/../../etc/passwd", "/a/b\0c"] {
            assert!(syntax_gate(bad).is_err(), "{bad} should be refused");
        }
        assert!(syntax_gate("/a/b/c.rs").is_ok());
    }

    #[test]
    fn a_path_outside_every_root_says_which_folders_there_are() {
        let dir = tempfile::tempdir().unwrap();
        let roots = roots(dir.path());
        let err = roots.resolve("/etc/passwd").unwrap_err();
        assert!(matches!(err, ScopeError::Outside { .. }), "{err}");
    }

    #[test]
    fn a_link_that_leaves_the_root_is_refused_at_the_open() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), b"s3cret").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            outside.path().join("secret.txt"),
            dir.path().join("link.txt"),
        )
        .unwrap();
        #[cfg(unix)]
        {
            let roots = roots(dir.path());
            let scoped = roots
                .resolve(&dir.path().join("link.txt").display().to_string())
                .expect("the path itself is inside the root");
            assert!(
                scoped.read().is_err(),
                "the open must refuse a link that leaves the folder"
            );
        }
    }

    #[test]
    fn gantrys_own_data_is_never_writable() {
        let dir = tempfile::tempdir().unwrap();
        let denied = dir.path().join("gantry");
        std::fs::create_dir_all(&denied).unwrap();
        let roots = Roots::open(
            &[dir.path().display().to_string()],
            std::slice::from_ref(&denied),
        );
        let err = roots
            .resolve(&denied.join("gantry.db").display().to_string())
            .unwrap_err();
        assert!(matches!(err, ScopeError::Denied(_)), "{err}");
    }

    #[test]
    fn a_write_replaces_the_file_in_one_step_and_keeps_its_mode() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, b"before").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o640)).unwrap();
        }
        let roots = roots(dir.path());
        let scoped = roots.resolve(&file.display().to_string()).unwrap();
        scoped.write_atomically(b"after").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"after");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&file).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o640);
        }
        // The temporary file is gone either way.
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().contains("gantry-"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }
}
