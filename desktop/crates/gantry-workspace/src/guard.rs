//! Files that hold credentials (`docs/connectors/filesystem.md` D3).
//!
//! Reading one of these is a legitimate question ("is this variable set?"). Editing one from a
//! model's own initiative is not, and until the guardrail floor of M7 can turn this into a
//! confirmation the user answers, the edit tools refuse and say so. Fail closed, and say which
//! rule closed.

use std::path::Path;

/// Why this path is sensitive, or `None`.
#[must_use]
pub fn sensitive(path: &Path) -> Option<&'static str> {
    const DIRECTORIES: [(&str, &str); 5] = [
        (".ssh", "an SSH key directory"),
        (".aws", "a cloud credential directory"),
        (".gnupg", "a GnuPG key directory"),
        (".kube", "a cluster credential directory"),
        (".docker", "a registry credential directory"),
    ];
    const NAMES: [(&str, &str); 6] = [
        (".netrc", "a stored login"),
        (".npmrc", "a registry token file"),
        (".pypirc", "a registry token file"),
        ("credentials", "a credential file"),
        ("id_rsa", "a private key"),
        ("id_ed25519", "a private key"),
    ];
    const EXTENSIONS: [(&str, &str); 5] = [
        ("pem", "a private key or certificate"),
        ("key", "a private key"),
        ("p12", "a key store"),
        ("pfx", "a key store"),
        ("keystore", "a key store"),
    ];

    for component in path.components() {
        let name = component.as_os_str().to_string_lossy();
        if let Some((_, why)) = DIRECTORIES.iter().find(|(d, _)| name == *d) {
            return Some(why);
        }
    }
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    if name == ".env" || name.starts_with(".env.") || name.ends_with(".env") {
        return Some("an environment file");
    }
    if let Some((_, why)) = NAMES.iter().find(|(n, _)| name == *n) {
        return Some(why);
    }
    let extension = path.extension()?.to_string_lossy().to_lowercase();
    EXTENSIONS
        .iter()
        .find(|(e, _)| extension == *e)
        .map(|(_, why)| *why)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_files_are_recognised_and_ordinary_source_is_not() {
        for path in [
            "/w/.env",
            "/w/.env.local",
            "/w/app/server.pem",
            "/home/o/.ssh/config",
            "/w/.aws/credentials",
        ] {
            assert!(sensitive(Path::new(path)).is_some(), "{path}");
        }
        for path in ["/w/src/main.rs", "/w/environment.ts", "/w/keys.md"] {
            assert!(sensitive(Path::new(path)).is_none(), "{path}");
        }
    }
}
