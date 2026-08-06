"""Resolving the account a run bills to.

Gantry has no signup step: identity arrives as a verified Supabase JWT, and the
first time an authenticated caller launches anything their ``users`` row is
created on demand with the configured starting grant. When auth is disabled
(local dev, CI) every run bills the seeded ``LOCAL_USER_ID`` account, so the
metering path behaves identically in every environment instead of only being
exercised in production.
"""

from __future__ import annotations

import uuid
from decimal import Decimal

import sqlalchemy as sa
from sqlalchemy.dialects.postgresql import insert as pg_insert
from sqlalchemy.ext.asyncio import AsyncSession

from gantry.core.models import DEFAULT_WORKSPACE_ID, LOCAL_USER_ID, User

#: Credits a brand-new account is granted. Small on purpose: enough to prove the
#: product works, not enough to be worth farming accounts for.
DEFAULT_STARTING_CREDITS = Decimal("1000")


async def resolve_user_id(
    session: AsyncSession,
    email: str | None,
    *,
    subject: str | None = None,
    workspace_id: uuid.UUID = DEFAULT_WORKSPACE_ID,
) -> uuid.UUID:
    """The account id for a caller, creating the row on first sight.

    ``email is None`` means auth is disabled, which resolves to the local
    account rather than to "no owner" — an unowned run would silently escape
    metering, and a billing system that can be bypassed by turning off auth is
    not a billing system.

    The upsert is ``ON CONFLICT DO NOTHING`` on ``(workspace_id, email)``, so two
    concurrent first-time launches by the same user cannot create two accounts
    (and race one another's balances).
    """
    if not email:
        return await ensure_local_user(session, workspace_id=workspace_id)

    normalized = email.strip().lower()
    await session.execute(
        pg_insert(User)
        .values(
            id=uuid.uuid4(),
            workspace_id=workspace_id,
            email=normalized,
            subject=(subject or "")[:128],
            gantry_credits_balance=DEFAULT_STARTING_CREDITS,
        )
        .on_conflict_do_nothing(constraint="uq_users_workspace_email")
    )
    user_id = (
        await session.execute(
            sa.select(User.id).where(User.workspace_id == workspace_id, User.email == normalized)
        )
    ).scalar_one()
    return user_id


async def ensure_local_user(
    session: AsyncSession, *, workspace_id: uuid.UUID = DEFAULT_WORKSPACE_ID
) -> uuid.UUID:
    """The fixed account used when auth is off. Idempotent.

    Migration 0015 seeds it; this re-creates it for a database that predates the
    seed or had it deleted, so an auth-less deployment can never hit a missing
    FK target mid-run.
    """
    await session.execute(
        pg_insert(User)
        .values(
            id=LOCAL_USER_ID,
            workspace_id=workspace_id,
            email="local@gantry.local",
            subject="",
            gantry_credits_balance=DEFAULT_STARTING_CREDITS,
        )
        .on_conflict_do_nothing(index_elements=[User.id])
    )
    return LOCAL_USER_ID


async def grant_credits(
    session: AsyncSession, user_id: uuid.UUID, credits: Decimal
) -> Decimal | None:
    """Top up a balance atomically. Returns the new balance, or None if unknown."""
    row = (
        await session.execute(
            sa.update(User)
            .where(User.id == user_id)
            .values(
                gantry_credits_balance=User.gantry_credits_balance + credits,
                updated_at=sa.func.now(),
            )
            .returning(User.gantry_credits_balance)
        )
    ).first()
    return None if row is None else Decimal(row[0])
