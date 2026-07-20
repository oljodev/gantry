"""Vault: AES-256-GCM roundtrips, key parsing, and the secrets store."""

from __future__ import annotations

import base64
import os

import pytest
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.config import Settings
from gantry.core.models import DEFAULT_WORKSPACE_ID
from gantry.vault import Vault, VaultError, last4
from gantry.vault.store import delete_secret, get_secret, put_secret, secret_status

WS = DEFAULT_WORKSPACE_ID


class TestVault:
    def test_roundtrip(self) -> None:
        vault = Vault(os.urandom(32))
        assert vault.decrypt(vault.encrypt("sk-super-secret")) == "sk-super-secret"

    def test_nonce_uniqueness(self) -> None:
        vault = Vault(os.urandom(32))
        assert vault.encrypt("same") != vault.encrypt("same")

    def test_wrong_key_fails(self) -> None:
        blob = Vault(os.urandom(32)).encrypt("secret")
        with pytest.raises(VaultError):
            Vault(os.urandom(32)).decrypt(blob)

    def test_truncated_blob_fails(self) -> None:
        with pytest.raises(VaultError):
            Vault(os.urandom(32)).decrypt(b"short")

    def test_bad_key_length(self) -> None:
        with pytest.raises(VaultError):
            Vault(b"too-short")

    def test_from_settings_hex_and_base64(self) -> None:
        key = os.urandom(32)
        for encoded in (key.hex(), base64.b64encode(key).decode()):
            settings = Settings(_env_file=None, vault_key=encoded)
            vault = Vault.from_settings(settings)
            assert vault.decrypt(Vault(key).encrypt("x")) == "x"

    def test_from_settings_missing_or_garbage(self, monkeypatch: pytest.MonkeyPatch) -> None:
        # litellm auto-loads .env into os.environ on import, and run.sh puts a
        # real GANTRY_VAULT_KEY there — clear it so "missing" is really missing.
        monkeypatch.delenv("GANTRY_VAULT_KEY", raising=False)
        with pytest.raises(VaultError):
            Vault.from_settings(Settings(_env_file=None))
        with pytest.raises(VaultError):
            Vault.from_settings(Settings(_env_file=None, vault_key="!!not-a-key!!"))
        with pytest.raises(VaultError):  # valid hex, wrong length
            Vault.from_settings(Settings(_env_file=None, vault_key="abcd"))

    def test_last4(self) -> None:
        assert last4("sk-abc123wxyz") == "wxyz"
        assert last4("abc") == ""


class TestSecretStore:
    async def test_put_get_roundtrip(self, db: async_sessionmaker[AsyncSession]) -> None:
        vault = Vault(os.urandom(32))
        async with db() as session, session.begin():
            hint = await put_secret(
                session, vault, workspace_id=WS, name="provider:x", plaintext="sk-12345678"
            )
        assert hint == "5678"
        async with db() as session:
            assert (
                await get_secret(session, vault, workspace_id=WS, name="provider:x")
                == "sk-12345678"
            )

    async def test_upsert_replaces(self, db: async_sessionmaker[AsyncSession]) -> None:
        vault = Vault(os.urandom(32))
        async with db() as session, session.begin():
            await put_secret(session, vault, workspace_id=WS, name="github:token", plaintext="old")
            await put_secret(
                session,
                vault,
                workspace_id=WS,
                name="github:token",
                plaintext="gho_new_token",
                meta={"login": "oljodev"},
            )
        async with db() as session:
            assert (
                await get_secret(session, vault, workspace_id=WS, name="github:token")
                == "gho_new_token"
            )
            row = await secret_status(session, workspace_id=WS, name="github:token")
            assert row is not None
            assert row.last4 == "oken"
            assert row.meta == {"login": "oljodev"}

    async def test_missing_and_delete(self, db: async_sessionmaker[AsyncSession]) -> None:
        vault = Vault(os.urandom(32))
        async with db() as session:
            assert await get_secret(session, vault, workspace_id=WS, name="nope") is None
        async with db() as session, session.begin():
            await put_secret(session, vault, workspace_id=WS, name="doomed", plaintext="x" * 10)
            assert await delete_secret(session, workspace_id=WS, name="doomed") is True
            assert await delete_secret(session, workspace_id=WS, name="doomed") is False
        async with db() as session:
            assert await secret_status(session, workspace_id=WS, name="doomed") is None

    async def test_ciphertext_does_not_contain_plaintext(
        self, db: async_sessionmaker[AsyncSession]
    ) -> None:
        vault = Vault(os.urandom(32))
        secret_value = "sk-proj-veryvisible"
        async with db() as session, session.begin():
            await put_secret(session, vault, workspace_id=WS, name="p", plaintext=secret_value)
        async with db() as session:
            row = await secret_status(session, workspace_id=WS, name="p")
            assert row is not None
            assert secret_value.encode() not in row.ciphertext
