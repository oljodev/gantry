"""Recording a call: one audit row + one atomic balance decrement.

The invariant this module exists to hold:

    users.gantry_credits_balance == starting_balance - SUM(credits_deducted)

Both writes happen in **one transaction**, so there is no window where a call is
logged but unbilled (free inference) or billed but unlogged (an unexplainable
balance). The decrement is a single ``UPDATE ... SET balance = balance - :c``
evaluated by Postgres, never a read-modify-write in Python: a swarm of 100 agents
charging one account concurrently would otherwise interleave reads and lose
charges, and losing charges is exactly the failure a credit system cannot have.

A balance is allowed to go **negative**. By the time we are called the provider
has already answered and already billed us; refusing to record the charge would
not un-spend the money, it would only hide it. Enforcement belongs *before* a
task starts (see :func:`has_credit`), not after a response arrives.
"""

from __future__ import annotations

import uuid
from dataclasses import dataclass
from decimal import Decimal

import sqlalchemy as sa
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.billing.credits import CallCharge, calculate_credits_for_call
from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.models import DEFAULT_WORKSPACE_ID, LlmUsageLog, User
from gantry.logging import get_logger
from gantry.runtime.llm import LLMUsage

logger = get_logger(__name__)

Sessions = async_sessionmaker[AsyncSession]


@dataclass(frozen=True)
class BillingContext:
    """Who and what a metered call is attributed to.

    Carried alongside the LLM client rather than looked up per call: the worker
    already knows all of this when it claims a task, and a DB round-trip per
    completion to re-derive it would be pure overhead.
    """

    workspace_id: uuid.UUID = DEFAULT_WORKSPACE_ID
    user_id: uuid.UUID | None = None
    run_id: uuid.UUID | None = None
    task_id: uuid.UUID | None = None


@dataclass(frozen=True)
class RecordedCall:
    """Outcome of billing one call."""

    log_id: uuid.UUID
    charge: CallCharge
    #: Balance AFTER the deduction, or None when the call had no owner.
    balance_after: Decimal | None


async def record_call(
    session: AsyncSession,
    context: BillingContext,
    *,
    model_slug: str,
    usage: LLMUsage,
) -> RecordedCall:
    """Log one LLM call and deduct its credits, atomically.

    Caller supplies the session so this composes into a larger transaction; the
    convenience wrapper :func:`record_call_in_session` opens its own.
    """
    charge = calculate_credits_for_call(
        model_slug,
        usage.prompt_tokens,
        usage.completion_tokens,
        cache_read_tokens=usage.cache_read_tokens,
        cache_write_tokens=usage.cache_write_tokens,
    )
    log = LlmUsageLog(
        id=uuid.uuid4(),
        workspace_id=context.workspace_id,
        user_id=context.user_id,
        run_id=context.run_id,
        task_id=context.task_id,
        model_slug=model_slug,
        prompt_tokens=charge.prompt_tokens,
        completion_tokens=charge.completion_tokens,
        total_tokens=charge.total_tokens,
        cache_read_tokens=charge.cache_read_tokens,
        cache_write_tokens=charge.cache_write_tokens,
        raw_cost_usd=charge.raw_cost_usd,
        credits_deducted=charge.credits_deducted,
    )
    session.add(log)

    balance_after: Decimal | None = None
    if context.user_id is not None and charge.credits_deducted != 0:
        balance_after = await deduct_credits(session, context.user_id, charge.credits_deducted)
    return RecordedCall(log_id=log.id, charge=charge, balance_after=balance_after)


async def deduct_credits(
    session: AsyncSession, user_id: uuid.UUID, credits: Decimal
) -> Decimal | None:
    """Atomically subtract ``credits`` from a balance; returns the new balance.

    One statement, evaluated in the database: the read and the write cannot be
    interleaved by another agent's deduction. None means the user row is gone.
    """
    row = (
        await session.execute(
            sa.update(User)
            .where(User.id == user_id)
            .values(
                gantry_credits_balance=User.gantry_credits_balance - credits,
                updated_at=sa.func.now(),
            )
            .returning(User.gantry_credits_balance)
        )
    ).first()
    return None if row is None else Decimal(row[0])


async def record_call_in_session(
    sessions: Sessions,
    context: BillingContext,
    *,
    model_slug: str,
    usage: LLMUsage,
) -> RecordedCall:
    """:func:`record_call` in its own committed transaction."""
    async with session_scope(sessions) as session:
        return await record_call(session, context, model_slug=model_slug, usage=usage)


async def credit_balance(session: AsyncSession, user_id: uuid.UUID) -> Decimal | None:
    """A user's current balance, or None if there is no such user."""
    return (
        await session.execute(sa.select(User.gantry_credits_balance).where(User.id == user_id))
    ).scalar_one_or_none()


async def has_credit(session: AsyncSession, user_id: uuid.UUID | None) -> bool:
    """Whether this account may start new work.

    The gate is ``balance > 0``, deliberately not "balance >= the cost of the next
    call": that cost is unknowable before the call is made. An account is allowed
    to overrun its last credit by one call and land slightly negative — bounded,
    visible, and far better than refusing to run a task we cannot price yet.
    A user with no row (unattributed work) is not gated.
    """
    if user_id is None:
        return True
    balance = await credit_balance(session, user_id)
    return balance is None or balance > 0


@dataclass(frozen=True)
class GrantOutcome:
    """What happened when credits were added to an account."""

    user_id: uuid.UUID
    balance_after: Decimal
    resumed_tasks: int


async def grant_credits_and_resume(
    session: AsyncSession, user_id: uuid.UUID, credits: Decimal
) -> GrantOutcome | None:
    """THE internal credit grant function — the one place a balance goes up AND
    whatever it unblocks gets woken.

    Both the admin/self-serve top-up route and the Paddle webhook call this
    exact function; neither hand-rolls its own UPDATE. (This is deliberately
    separate from :func:`gantry.billing.users.grant_credits`, which is the bare
    balance-only primitive test setup uses — that one has no opinion about
    paused runs, this one is the full user-facing operation.) Two things happen
    in one transaction, not two:

    1. the balance is incremented atomically (``UPDATE ... balance + :c``, the
       same interleaving-safe shape as the deduction path);
    2. any run this account paused for lack of credit (``PAUSED_OUT_OF_CREDITS``)
       is woken.

    Splitting those into separate calls would leave a real window where a
    successful payment has landed but the swarm it was meant to fund is still
    sitting parked — from the user's side, indistinguishable from the payment
    having silently failed. Returns ``None`` if ``user_id`` does not exist,
    which the caller turns into a 404 rather than crediting nothing and calling
    it a success.
    """
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
    if row is None:
        return None
    woken = await queue.resume_paused_accounts(session, user_id=user_id)
    return GrantOutcome(user_id=user_id, balance_after=Decimal(row[0]), resumed_tasks=woken)


async def run_credits_used(session: AsyncSession, run_id: uuid.UUID) -> Decimal:
    """Credits charged so far by a whole run (root task + every spawned child)."""
    total = (
        await session.execute(
            sa.select(sa.func.coalesce(sa.func.sum(LlmUsageLog.credits_deducted), 0)).where(
                LlmUsageLog.run_id == run_id
            )
        )
    ).scalar_one()
    return Decimal(total)
