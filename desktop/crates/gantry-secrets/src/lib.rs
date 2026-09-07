//! Secrets (docs/plan/06 §5): one 32-byte master key in the OS credential store, every secret
//! encrypted with XChaCha20-Poly1305 and stored as ciphertext in the `credentials` table.
//! Plaintext exists only inside this crate, briefly, and is zeroized after use.

#![forbid(unsafe_code)]

mod envelope;
mod master_key;
mod vault;

pub use master_key::SecretStoreStatus;
pub use secrecy::{ExposeSecret, SecretString};
pub use vault::{CredentialKind, OwnerKind, SecretRef, SecretVault, SecretsError};

/// The plan document that specifies this crate.
pub const PLAN_DOCUMENT: &str = "docs/plan/06-data-model.md";
