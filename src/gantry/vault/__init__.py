"""Encrypted secrets vault (AES-256-GCM).

Credentials (LLM API keys, the GitHub token) are stored in the ``secrets``
table as ``nonce(12) || ciphertext`` blobs; the single 32-byte key lives in
``GANTRY_VAULT_KEY`` outside the database, so a DB dump alone reveals nothing.
Losing the key makes all stored secrets unrecoverable — by design.
"""

from __future__ import annotations

import base64
import binascii
import os

from cryptography.exceptions import InvalidTag
from cryptography.hazmat.primitives.ciphers.aead import AESGCM

from gantry.config import Settings

_NONCE_SIZE = 12
_KEY_SIZE = 32


class VaultError(RuntimeError):
    """Missing/malformed key or undecryptable ciphertext."""


def _parse_key(raw: str) -> bytes:
    """Accept a 32-byte key as hex (64 chars) or standard base64."""
    text = raw.strip()
    try:
        key = bytes.fromhex(text)
    except ValueError:
        try:
            key = base64.b64decode(text, validate=True)
        except (binascii.Error, ValueError) as exc:
            raise VaultError("GANTRY_VAULT_KEY is neither valid hex nor base64") from exc
    if len(key) != _KEY_SIZE:
        raise VaultError(f"GANTRY_VAULT_KEY must decode to {_KEY_SIZE} bytes, got {len(key)}")
    return key


class Vault:
    def __init__(self, key: bytes) -> None:
        if len(key) != _KEY_SIZE:
            raise VaultError(f"vault key must be {_KEY_SIZE} bytes, got {len(key)}")
        self._aead = AESGCM(key)

    @classmethod
    def from_settings(cls, settings: Settings) -> Vault:
        if not settings.vault_key:
            raise VaultError("GANTRY_VAULT_KEY is not set — cannot use the secrets vault")
        return cls(_parse_key(settings.vault_key))

    def encrypt(self, plaintext: str) -> bytes:
        nonce = os.urandom(_NONCE_SIZE)
        return nonce + self._aead.encrypt(nonce, plaintext.encode("utf-8"), None)

    def decrypt(self, blob: bytes) -> str:
        if len(blob) <= _NONCE_SIZE:
            raise VaultError("ciphertext too short")
        try:
            plain = self._aead.decrypt(blob[:_NONCE_SIZE], blob[_NONCE_SIZE:], None)
        except InvalidTag as exc:
            raise VaultError("decryption failed — wrong key or corrupted ciphertext") from exc
        return plain.decode("utf-8")


def last4(plaintext: str) -> str:
    """Display hint for a stored secret (never enough to reconstruct it)."""
    return plaintext[-4:] if len(plaintext) >= 4 else ""
