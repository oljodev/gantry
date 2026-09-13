//! The vault: the only place plaintext secrets exist. Callers get a [`SecretRef`] (id, hint)
//! for the UI and a [`SecretString`] for the one client that needs the value.

use std::{path::Path, sync::Arc};

use gantry_store::{Store, repos::credentials};
use secrecy::SecretString;
use zeroize::Zeroizing;

use crate::{
    envelope,
    master_key::{self, MasterKey, MasterKeyError, SecretStoreStatus},
};

#[derive(Debug, thiserror::Error)]
pub enum SecretsError {
    #[error("{0}")]
    MasterKey(#[from] MasterKeyError),
    #[error("{0}")]
    Envelope(#[from] envelope::EnvelopeError),
    #[error("{0}")]
    Store(#[from] gantry_store::StoreError),
    #[error("credential not found")]
    NotFound,
}

impl From<SecretsError> for gantry_core::GantryError {
    fn from(err: SecretsError) -> Self {
        match err {
            SecretsError::NotFound => gantry_core::GantryError::NotFound("credential".into()),
            other => gantry_core::GantryError::Secrets(other.to_string()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerKind {
    Provider,
    Instance,
}

impl OwnerKind {
    fn as_str(self) -> &'static str {
        match self {
            OwnerKind::Provider => "provider",
            OwnerKind::Instance => "instance",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialKind {
    ApiKey,
    OauthToken,
    OauthClientSecret,
    UserConfigSecret,
    SearchApiKey,
}

impl CredentialKind {
    /// The string the credential is filed under. Public because a reader has to ask for the
    /// kind it wants back — `user_config_secret` and nothing else, when resolving a form answer.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            CredentialKind::ApiKey => "api_key",
            CredentialKind::OauthToken => "oauth_token",
            CredentialKind::OauthClientSecret => "oauth_client_secret",
            CredentialKind::UserConfigSecret => "user_config_secret",
            CredentialKind::SearchApiKey => "search_api_key",
        }
    }
}

/// What the rest of the app may know about a secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretRef {
    pub id: String,
    pub kind: String,
    pub label: Option<String>,
    /// The last four characters of the plaintext, for "set ····abcd".
    pub hint: String,
}

pub struct SecretVault {
    key: MasterKey,
    store: Arc<Store>,
    status: SecretStoreStatus,
}

impl std::fmt::Debug for SecretVault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretVault")
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

/// The last four characters, or everything when shorter.
fn hint_of(plaintext: &str) -> String {
    let chars: Vec<char> = plaintext.chars().collect();
    let start = chars.len().saturating_sub(4);
    chars[start..].iter().collect()
}

impl SecretVault {
    /// Loads or creates the master key and binds the vault to the store. Blocking; call it on a
    /// plain thread at startup.
    pub fn open(data_dir: &Path, store: Arc<Store>) -> Result<SecretVault, SecretsError> {
        let (key, status) = master_key::load_or_create(data_dir)?;
        Ok(SecretVault { key, store, status })
    }

    /// A vault on a caller-supplied key; for tests and for key rotation.
    #[must_use]
    pub fn with_key(
        key: [u8; envelope::KEY_LEN],
        store: Arc<Store>,
        status: SecretStoreStatus,
    ) -> SecretVault {
        SecretVault {
            key: MasterKey(Zeroizing::new(key)),
            store,
            status,
        }
    }

    #[must_use]
    pub fn status(&self) -> &SecretStoreStatus {
        &self.status
    }

    /// Encrypts and stores a new secret, replacing the existing one with the same kind *and*
    /// label for the same owner. Returns what the UI may see.
    pub async fn set(
        &self,
        owner: OwnerKind,
        owner_id: &str,
        kind: CredentialKind,
        label: Option<&str>,
        plaintext: &str,
    ) -> Result<SecretRef, SecretsError> {
        let id = ulid::Ulid::new().to_string();
        let hint = hint_of(plaintext);
        let aad = envelope::aad(&id, kind.as_str());
        let (nonce, ciphertext) = envelope::encrypt(&self.key.0, &aad, plaintext.as_bytes())?;
        let now = gantry_core::now_ms();
        let record = credentials::CredentialRecord {
            id: id.clone(),
            kind: kind.as_str().to_owned(),
            owner_kind: owner.as_str().to_owned(),
            owner_id: owner_id.to_owned(),
            label: label.map(str::to_owned),
            ciphertext,
            nonce: nonce.to_vec(),
            expires_at: None,
            meta_json: serde_json::json!({ "hint": hint }).to_string(),
            created_at: now,
            updated_at: now,
        };
        let owner_kind = owner.as_str().to_owned();
        let owner_id_owned = owner_id.to_owned();
        let kind_str = kind.as_str().to_owned();
        let label_owned = label.map(str::to_owned);
        self.store
            .write(move |conn| {
                // Replace the credential this one *is*, not every credential of its kind. The
                // label is what tells two apart, and one owner may hold several of a kind: a
                // connector whose `user_config` has two sensitive fields keeps one secret per
                // field, and matching on the kind alone deleted the first when the second was
                // saved (06 §5).
                for old in credentials::list_for_owner(conn, &owner_kind, &owner_id_owned)? {
                    if old.kind == kind_str && old.label == label_owned {
                        credentials::delete(conn, &old.id)?;
                    }
                }
                credentials::insert(conn, &record)
            })
            .await?;
        Ok(SecretRef {
            id,
            kind: kind.as_str().to_owned(),
            label: label.map(str::to_owned),
            hint,
        })
    }

    /// Decrypts one secret. The value lives in a [`SecretString`] and is zeroized on drop.
    pub fn get(&self, id: &str) -> Result<SecretString, SecretsError> {
        let record = self
            .store
            .read(|conn| credentials::get(conn, id))?
            .ok_or(SecretsError::NotFound)?;
        let aad = envelope::aad(&record.id, &record.kind);
        let bytes = envelope::decrypt(&self.key.0, &aad, &record.nonce, &record.ciphertext)?;
        let text = String::from_utf8(bytes.to_vec())
            .map_err(|_| envelope::EnvelopeError::from_static("secret is not text"))?;
        Ok(SecretString::from(text))
    }

    /// The secrets of one owner, without their values.
    pub fn list_for_owner(
        &self,
        owner: OwnerKind,
        owner_id: &str,
    ) -> Result<Vec<SecretRef>, SecretsError> {
        let rows = self
            .store
            .read(|conn| credentials::list_for_owner(conn, owner.as_str(), owner_id))?;
        Ok(rows.into_iter().map(to_ref).collect())
    }

    pub async fn delete(&self, id: &str) -> Result<(), SecretsError> {
        let id = id.to_owned();
        self.store
            .write(move |conn| credentials::delete(conn, &id))
            .await?;
        Ok(())
    }
}

fn to_ref(r: credentials::CredentialRecord) -> SecretRef {
    let hint = serde_json::from_str::<serde_json::Value>(&r.meta_json)
        .ok()
        .and_then(|m| m.get("hint").and_then(|h| h.as_str()).map(str::to_owned))
        .unwrap_or_default();
    SecretRef {
        id: r.id,
        kind: r.kind,
        label: r.label,
        hint,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;

    fn vault() -> (tempfile::TempDir, SecretVault) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(dir.path().join("t.db")).unwrap());
        let key = envelope::random_bytes::<32>().unwrap();
        let status = SecretStoreStatus::FileFallback {
            path: "test".into(),
        };
        (dir, SecretVault::with_key(key, store, status))
    }

    #[tokio::test]
    async fn set_get_replace_delete() {
        let (_dir, vault) = vault();
        let a = vault
            .set(
                OwnerKind::Provider,
                "openrouter",
                CredentialKind::ApiKey,
                Some("k"),
                "sk-or-abcd1234",
            )
            .await
            .unwrap();
        assert_eq!(a.hint, "1234");
        assert_eq!(vault.get(&a.id).unwrap().expose_secret(), "sk-or-abcd1234");

        let b = vault
            .set(
                OwnerKind::Provider,
                "openrouter",
                CredentialKind::ApiKey,
                Some("k"),
                "sk-or-wxyz9876",
            )
            .await
            .unwrap();
        let refs = vault
            .list_for_owner(OwnerKind::Provider, "openrouter")
            .unwrap();
        assert_eq!(
            refs.len(),
            1,
            "the same kind under the same label is replaced"
        );
        assert_eq!(refs[0].id, b.id);
        assert!(matches!(vault.get(&a.id), Err(SecretsError::NotFound)));

        // But a second label of the same kind is a second secret, not a replacement: a
        // connector whose `user_config` has two sensitive fields keeps one per field, and
        // matching on the kind alone deleted the first when the second was saved.
        let other = vault
            .set(
                OwnerKind::Provider,
                "openrouter",
                CredentialKind::ApiKey,
                Some("second"),
                "sk-or-0000zzzz",
            )
            .await
            .unwrap();
        let refs = vault
            .list_for_owner(OwnerKind::Provider, "openrouter")
            .unwrap();
        assert_eq!(refs.len(), 2, "two labels, two secrets");
        assert_eq!(
            vault.get(&other.id).unwrap().expose_secret(),
            "sk-or-0000zzzz"
        );
        assert_eq!(vault.get(&b.id).unwrap().expose_secret(), "sk-or-wxyz9876");
        vault.delete(&other.id).await.unwrap();

        vault.delete(&b.id).await.unwrap();
        assert!(
            vault
                .list_for_owner(OwnerKind::Provider, "openrouter")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_short_secret_hints_all_of_it() {
        assert_eq!(hint_of("ab"), "ab");
        assert_eq!(hint_of("abcdef"), "cdef");
    }
}
