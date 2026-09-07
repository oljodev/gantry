//! The master key: generated once, kept in the OS credential store (service `dev.oljo.gantry`,
//! user `master-key`, base64). Linux without a Secret Service falls back to a 0600 file and
//! says so (01 §6, 06 §5).

use std::path::{Path, PathBuf};

use base64::{Engine, engine::general_purpose::STANDARD as B64};
use keyring_core::Entry;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::envelope::{KEY_LEN, random_bytes};

const SERVICE: &str = "dev.oljo.gantry";
const USER: &str = "master-key";
const FALLBACK_FILE: &str = "master.key";

/// Where the master key lives; shown in Settings → Providers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SecretStoreStatus {
    /// The operating system's credential store.
    OsStore { backend: String },
    /// A file in the data directory, readable by the user only. Linux without Secret Service.
    FileFallback { path: String },
}

pub struct MasterKey(pub(crate) Zeroizing<[u8; KEY_LEN]>);

#[derive(Debug, thiserror::Error)]
pub enum MasterKeyError {
    #[error("credential store: {0}")]
    Keyring(#[from] keyring_core::Error),
    #[error("{0}")]
    Envelope(#[from] crate::envelope::EnvelopeError),
    #[error("stored master key is not valid: {0}")]
    Corrupt(&'static str),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Loads the master key, creating it on first run. Tries the OS store first; on Linux, falls
/// back to the key file when no Secret Service answers.
pub fn load_or_create(data_dir: &Path) -> Result<(MasterKey, SecretStoreStatus), MasterKeyError> {
    match os_store::install() {
        Ok(backend) => match load_or_create_in_os_store() {
            Ok(key) => return Ok((key, SecretStoreStatus::OsStore { backend })),
            Err(err) if cfg!(target_os = "linux") => {
                log::warn!("OS credential store unusable ({err}); using the key file");
            }
            Err(err) => return Err(err),
        },
        Err(err) if cfg!(target_os = "linux") => {
            log::warn!("no OS credential store ({err}); using the key file");
        }
        Err(err) => return Err(err.into()),
    }
    let path = data_dir.join(FALLBACK_FILE);
    let key = load_or_create_in_file(&path)?;
    Ok((
        key,
        SecretStoreStatus::FileFallback {
            path: path.display().to_string(),
        },
    ))
}

fn decode(encoded: &str) -> Result<MasterKey, MasterKeyError> {
    let bytes = Zeroizing::new(
        B64.decode(encoded.trim())
            .map_err(|_| MasterKeyError::Corrupt("not base64"))?,
    );
    let arr: [u8; KEY_LEN] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| MasterKeyError::Corrupt("wrong length"))?;
    Ok(MasterKey(Zeroizing::new(arr)))
}

fn load_or_create_in_os_store() -> Result<MasterKey, MasterKeyError> {
    let entry = Entry::new(SERVICE, USER)?;
    match entry.get_password() {
        Ok(encoded) => decode(&encoded),
        Err(keyring_core::Error::NoEntry) => {
            let fresh = random_bytes::<KEY_LEN>()?;
            entry.set_password(&B64.encode(fresh))?;
            log::info!("created the master key in the OS credential store");
            Ok(MasterKey(Zeroizing::new(fresh)))
        }
        Err(err) => Err(err.into()),
    }
}

fn load_or_create_in_file(path: &PathBuf) -> Result<MasterKey, MasterKeyError> {
    match std::fs::read_to_string(path) {
        Ok(encoded) => decode(&encoded),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let fresh = random_bytes::<KEY_LEN>()?;
            write_private(path, B64.encode(fresh).as_bytes())?;
            log::info!("created the master key file at {}", path.display());
            Ok(MasterKey(Zeroizing::new(fresh)))
        }
        Err(err) => Err(err.into()),
    }
}

#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(bytes)
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)
}

mod os_store {
    //! Installing the platform store into `keyring-core`. Returns the backend's name.
    use keyring_core::Error;

    #[cfg(target_os = "macos")]
    pub fn install() -> Result<String, Error> {
        keyring_core::set_default_store(apple_native_keyring_store::keychain::Store::new()?);
        Ok("macOS Keychain".into())
    }

    #[cfg(windows)]
    pub fn install() -> Result<String, Error> {
        keyring_core::set_default_store(windows_native_keyring_store::Store::new()?);
        Ok("Windows Credential Manager".into())
    }

    #[cfg(target_os = "linux")]
    pub fn install() -> Result<String, Error> {
        keyring_core::set_default_store(zbus_secret_service_keyring_store::Store::new()?);
        Ok("Secret Service".into())
    }

    #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
    pub fn install() -> Result<String, Error> {
        Err(Error::NoDefaultStore)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_fallback_is_stable_across_loads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FALLBACK_FILE);
        let a = load_or_create_in_file(&path).unwrap();
        let b = load_or_create_in_file(&path).unwrap();
        assert_eq!(*a.0, *b.0);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn a_corrupt_file_is_reported_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FALLBACK_FILE);
        std::fs::write(&path, "not-a-key").unwrap();
        assert!(matches!(
            load_or_create_in_file(&path),
            Err(MasterKeyError::Corrupt(_))
        ));
    }
}
