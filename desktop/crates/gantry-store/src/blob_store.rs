//! Content-addressed files under `<data_dir>/blobs/ab/abcd…` (docs/plan/06 §1). Immutable,
//! deduplicated by SHA-256; the `blobs` table carries size, mime and the reference count.

use std::{
    fs,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

use crate::db::{Result, StoreError};

#[derive(Debug, Clone)]
pub struct BlobStore {
    root: PathBuf,
}

impl BlobStore {
    /// `root` is the `blobs/` directory; it is created when missing.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The lowercase hex SHA-256 of `bytes`.
    #[must_use]
    pub fn hash_of(bytes: &[u8]) -> String {
        let digest = Sha256::digest(bytes);
        digest.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[must_use]
    pub fn path_for(&self, hash: &str) -> PathBuf {
        let prefix = hash.get(..2).unwrap_or("00");
        self.root.join(prefix).join(hash)
    }

    /// Writes the file when it does not exist yet and returns its hash. The caller records the
    /// reference in the `blobs` table.
    ///
    /// A file that is already there is left alone but its modified time is moved to now, which
    /// is what makes the sweep's grace period work (`crate::sweep`): the time on a blob means
    /// "when these bytes were last handed out", and a blob handed out a moment ago is a blob
    /// whose reference is about to be written.
    pub fn put(&self, bytes: &[u8]) -> Result<String> {
        let hash = Self::hash_of(bytes);
        let path = self.path_for(&hash);
        if path.exists() {
            touch(&path);
            return Ok(hash);
        }
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("part");
        fs::write(&tmp, bytes)?;
        fs::rename(&tmp, &path)?;
        Ok(hash)
    }

    pub fn get(&self, hash: &str) -> Result<Vec<u8>> {
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(StoreError::Other(format!("not a blob hash: {hash}")));
        }
        Ok(fs::read(self.path_for(hash))?)
    }

    pub fn remove(&self, hash: &str) -> Result<()> {
        let path = self.path_for(hash);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

/// Moves a file's modified time to now, without rewriting it. Not being able to is not worth
/// failing a put over: the cost is that the sweep may collect the blob a few minutes early, and
/// the caller is about to write the reference that stops it.
fn touch(path: &Path) {
    if let Ok(file) = fs::OpenOptions::new().write(true).open(path)
        && let Err(err) = file.set_modified(std::time::SystemTime::now())
    {
        log::debug!("could not touch {}: {err}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn puts_are_idempotent_and_readable() {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::open(dir.path().join("blobs")).unwrap();
        let a = store.put(b"hello").unwrap();
        let b = store.put(b"hello").unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert_eq!(store.get(&a).unwrap(), b"hello");
        assert!(
            store
                .path_for(&a)
                .starts_with(dir.path().join("blobs").join(&a[..2]))
        );
        assert!(store.get("nope").is_err());
        store.remove(&a).unwrap();
        assert!(store.get(&a).is_err());
    }
}
