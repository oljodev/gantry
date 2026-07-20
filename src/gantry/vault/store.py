"""Async DB helpers for vault-encrypted secrets.

Secrets are addressed by ``(workspace_id, name)`` where ``name`` follows the
conventions ``github:token`` and ``provider:{provider_id}``. All helpers
operate inside the caller's transaction (no commit here).
"""

from __future__ import annotations

import uuid
from datetime import UTC, datetime
from typing import Any, cast

import sqlalchemy as sa
from sqlalchemy.engine import CursorResult
from sqlalchemy.ext.asyncio import AsyncSession

from gantry.core.models import Secret
from gantry.vault import Vault, last4

GITHUB_TOKEN_SECRET = "github:token"


def provider_secret_name(provider_id: uuid.UUID) -> str:
    return f"provider:{provider_id}"


async def put_secret(
    session: AsyncSession,
    vault: Vault,
    *,
    workspace_id: uuid.UUID,
    name: str,
    plaintext: str,
    meta: dict[str, Any] | None = None,
) -> str:
    """Insert or replace a secret; returns the display ``last4``."""
    hint = last4(plaintext)
    existing = await _get_row(session, workspace_id=workspace_id, name=name)
    if existing is None:
        session.add(
            Secret(
                workspace_id=workspace_id,
                name=name,
                ciphertext=vault.encrypt(plaintext),
                last4=hint,
                meta=meta or {},
            )
        )
    else:
        existing.ciphertext = vault.encrypt(plaintext)
        existing.last4 = hint
        if meta is not None:
            existing.meta = meta
        existing.updated_at = datetime.now(UTC)
    await session.flush()
    return hint


async def get_secret(
    session: AsyncSession, vault: Vault, *, workspace_id: uuid.UUID, name: str
) -> str | None:
    row = await _get_row(session, workspace_id=workspace_id, name=name)
    return vault.decrypt(row.ciphertext) if row is not None else None


async def delete_secret(session: AsyncSession, *, workspace_id: uuid.UUID, name: str) -> bool:
    result = await session.execute(
        sa.delete(Secret).where(Secret.workspace_id == workspace_id, Secret.name == name)
    )
    return bool(cast("CursorResult[Any]", result).rowcount)


async def secret_status(
    session: AsyncSession, *, workspace_id: uuid.UUID, name: str
) -> Secret | None:
    """The secret row (last4 + meta) WITHOUT decrypting — for status endpoints."""
    return await _get_row(session, workspace_id=workspace_id, name=name)


async def _get_row(session: AsyncSession, *, workspace_id: uuid.UUID, name: str) -> Secret | None:
    result = await session.execute(
        sa.select(Secret).where(Secret.workspace_id == workspace_id, Secret.name == name)
    )
    return result.scalar_one_or_none()
