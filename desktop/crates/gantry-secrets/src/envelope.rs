//! XChaCha20-Poly1305 with a random 24-byte nonce and the credential's id and kind as
//! associated data, so a ciphertext cannot be moved to another row unnoticed.

use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use zeroize::Zeroizing;

pub const KEY_LEN: usize = 32;
pub const NONCE_LEN: usize = 24;

#[derive(Debug, thiserror::Error)]
#[error("envelope: {0}")]
pub struct EnvelopeError(&'static str);

impl EnvelopeError {
    pub(crate) fn from_static(msg: &'static str) -> Self {
        Self(msg)
    }
}

pub fn random_bytes<const N: usize>() -> Result<[u8; N], EnvelopeError> {
    let mut buf = [0u8; N];
    getrandom::fill(&mut buf).map_err(|_| EnvelopeError("no randomness available"))?;
    Ok(buf)
}

pub fn aad(id: &str, kind: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(id.len() + kind.len() + 1);
    v.extend_from_slice(id.as_bytes());
    v.push(0);
    v.extend_from_slice(kind.as_bytes());
    v
}

pub fn encrypt(
    key: &[u8; KEY_LEN],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<([u8; NONCE_LEN], Vec<u8>), EnvelopeError> {
    let nonce = random_bytes::<NONCE_LEN>()?;
    let cipher = XChaCha20Poly1305::new(key.into());
    let ciphertext = cipher
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| EnvelopeError("encryption failed"))?;
    Ok((nonce, ciphertext))
}

pub fn decrypt(
    key: &[u8; KEY_LEN],
    aad: &[u8],
    nonce: &[u8],
    ciphertext: &[u8],
) -> Result<Zeroizing<Vec<u8>>, EnvelopeError> {
    let nonce = XNonce::try_from(nonce).map_err(|_| EnvelopeError("bad nonce length"))?;
    let cipher = XChaCha20Poly1305::new(key.into());
    cipher
        .decrypt(
            &nonce,
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| EnvelopeError("decryption failed: wrong key or tampered data"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_binds_the_aad() {
        let key = random_bytes::<KEY_LEN>().unwrap();
        let a = aad("cred-1", "api_key");
        let (nonce, ct) = encrypt(&key, &a, b"sk-secret").unwrap();
        assert_eq!(&*decrypt(&key, &a, &nonce, &ct).unwrap(), b"sk-secret");
        assert!(decrypt(&key, &aad("cred-2", "api_key"), &nonce, &ct).is_err());
        let other = random_bytes::<KEY_LEN>().unwrap();
        assert!(decrypt(&other, &a, &nonce, &ct).is_err());
    }
}
