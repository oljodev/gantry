"""The ledger: every simulated LLM call is logged exactly, and the credits it
costs come off the balance atomically.

The invariant under test throughout is

    balance == starting_balance - SUM(credits_deducted)

which must survive concurrency, failures, and swarms of agents billing one
account at once. A billing system that loses charges under load is worse than
none, because the loss is invisible.
"""

from __future__ import annotations

import asyncio
import uuid
from decimal import Decimal

import sqlalchemy as sa
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.billing.catalog import CATALOG, TokenPrice
from gantry.billing.ledger import (
    BillingContext,
    credit_balance,
    has_credit,
    record_call_in_session,
    run_credits_used,
)
from gantry.billing.metering import MeteredLLMClient
from gantry.billing.users import grant_credits, resolve_user_id
from gantry.core.db import session_scope
from gantry.core.models import DEFAULT_WORKSPACE_ID, LlmUsageLog, User
from gantry.runtime.llm import LLMResponse, LLMUsage

from .fakes import ScriptedLLM

Sessions = async_sessionmaker[AsyncSession]

#: A deliberately round price so every expected charge below is exact:
#: $10/1M in, $30/1M out, $1/1M cached.
TEST_PRICE = TokenPrice(Decimal(10), Decimal(30), Decimal(1))
TEST_MODEL = "testvendor/fixture-model"


def _use_test_price() -> None:
    CATALOG.load({TEST_MODEL: TEST_PRICE})


async def _make_user(db: Sessions, credits: str = "1000") -> uuid.UUID:
    email = f"user-{uuid.uuid4().hex[:8]}@example.com"
    async with session_scope(db) as session:
        user_id = await resolve_user_id(session, email)
        await session.execute(
            sa.update(User)
            .where(User.id == user_id)
            .values(gantry_credits_balance=Decimal(credits))
        )
    return user_id


async def _logs_for(db: Sessions, user_id: uuid.UUID) -> list[LlmUsageLog]:
    async with db() as session:
        return list(
            (
                await session.scalars(
                    sa.select(LlmUsageLog)
                    .where(LlmUsageLog.user_id == user_id)
                    .order_by(LlmUsageLog.created_at)
                )
            ).all()
        )


# --- exact logging -------------------------------------------------------


async def test_a_call_is_logged_with_the_exact_tokens_and_charge(db: Sessions) -> None:
    _use_test_price()
    user_id = await _make_user(db)
    run_id, task_id = uuid.uuid4(), uuid.uuid4()

    await record_call_in_session(
        db,
        BillingContext(
            workspace_id=DEFAULT_WORKSPACE_ID, user_id=user_id, run_id=run_id, task_id=task_id
        ),
        model_slug=TEST_MODEL,
        usage=LLMUsage(prompt_tokens=100_000, completion_tokens=10_000),
    )

    (log,) = await _logs_for(db, user_id)
    assert log.model_slug == TEST_MODEL
    assert log.prompt_tokens == 100_000
    assert log.completion_tokens == 10_000
    assert log.total_tokens == 110_000
    assert log.run_id == run_id
    assert log.task_id == task_id
    assert log.workspace_id == DEFAULT_WORKSPACE_ID
    # 100K x $10/1M + 10K x $30/1M = $1.00 + $0.30
    assert log.raw_cost_usd == Decimal("1.30000000")
    # $1.30 / 0.6 x 100 GC = 216.666667 GC
    assert log.credits_deducted == Decimal("216.666667")


async def test_the_balance_matches_the_sum_of_every_logged_charge(db: Sessions) -> None:
    _use_test_price()
    user_id = await _make_user(db, "1000")
    ctx = BillingContext(workspace_id=DEFAULT_WORKSPACE_ID, user_id=user_id, run_id=uuid.uuid4())

    for _ in range(7):
        await record_call_in_session(
            db,
            ctx,
            model_slug=TEST_MODEL,
            usage=LLMUsage(prompt_tokens=1234, completion_tokens=567),
        )

    logs = await _logs_for(db, user_id)
    assert len(logs) == 7
    charged = sum((log.credits_deducted for log in logs), Decimal(0))
    async with db() as session:
        balance = await credit_balance(session, user_id)
    assert balance == Decimal(1000) - charged


async def test_cache_reads_reach_the_log_and_lower_the_charge(db: Sessions) -> None:
    _use_test_price()
    user_id = await _make_user(db)
    ctx = BillingContext(workspace_id=DEFAULT_WORKSPACE_ID, user_id=user_id)

    await record_call_in_session(
        db,
        ctx,
        model_slug=TEST_MODEL,
        usage=LLMUsage(prompt_tokens=1_000_000, completion_tokens=0, cache_read_tokens=900_000),
    )
    (log,) = await _logs_for(db, user_id)
    assert log.cache_read_tokens == 900_000
    # 100K fresh @ $10/1M + 900K cached @ $1/1M = $1.00 + $0.90
    assert log.raw_cost_usd == Decimal("1.90000000")


async def test_a_zero_usage_response_writes_no_row(db: Sessions) -> None:
    """A no-op completion is not a billable event; a zero row could never explain
    a balance change and would only bloat the audit trail."""
    _use_test_price()
    user_id = await _make_user(db)
    client = MeteredLLMClient(
        ScriptedLLM([LLMResponse(content="hi", usage=LLMUsage())]),
        db,
        BillingContext(workspace_id=DEFAULT_WORKSPACE_ID, user_id=user_id),
    )
    await client.complete(model=TEST_MODEL, messages=[{"role": "user", "content": "hi"}])
    assert await _logs_for(db, user_id) == []


async def test_an_unowned_call_is_still_logged(db: Sessions) -> None:
    """Work enqueued outside the API has no owner, but its usage must not vanish —
    unlogged spend is spend nobody can account for."""
    _use_test_price()
    run_id = uuid.uuid4()
    await record_call_in_session(
        db,
        BillingContext(workspace_id=DEFAULT_WORKSPACE_ID, user_id=None, run_id=run_id),
        model_slug=TEST_MODEL,
        usage=LLMUsage(prompt_tokens=1000, completion_tokens=100),
    )
    async with db() as session:
        assert await run_credits_used(session, run_id) > 0


# --- atomicity -----------------------------------------------------------


async def test_concurrent_charges_do_not_lose_deductions(db: Sessions) -> None:
    """The swarm case: many agents on one account at once.

    A read-modify-write in Python would interleave here and silently drop
    charges. The deduction is a single UPDATE evaluated by Postgres, so all 25
    land.
    """
    _use_test_price()
    user_id = await _make_user(db, "10000")
    ctx = BillingContext(workspace_id=DEFAULT_WORKSPACE_ID, user_id=user_id, run_id=uuid.uuid4())

    await asyncio.gather(
        *(
            record_call_in_session(
                db,
                ctx,
                model_slug=TEST_MODEL,
                usage=LLMUsage(prompt_tokens=10_000, completion_tokens=1_000),
            )
            for _ in range(25)
        )
    )

    logs = await _logs_for(db, user_id)
    assert len(logs) == 25
    charged = sum((log.credits_deducted for log in logs), Decimal(0))
    async with db() as session:
        balance = await credit_balance(session, user_id)
    assert balance == Decimal(10_000) - charged


async def test_the_log_row_and_the_deduction_commit_together(db: Sessions) -> None:
    """If the transaction rolls back, neither the row nor the charge survives —
    a logged-but-unbilled call is free inference, the reverse is an unexplainable
    balance."""
    _use_test_price()
    user_id = await _make_user(db, "500")
    ctx = BillingContext(workspace_id=DEFAULT_WORKSPACE_ID, user_id=user_id)

    class Boom(Exception):
        pass

    try:
        async with session_scope(db) as session:
            from gantry.billing.ledger import record_call

            await record_call(
                session,
                ctx,
                model_slug=TEST_MODEL,
                usage=LLMUsage(prompt_tokens=50_000, completion_tokens=5_000),
            )
            raise Boom
    except Boom:
        pass

    assert await _logs_for(db, user_id) == []
    async with db() as session:
        assert await credit_balance(session, user_id) == Decimal(500)


async def test_a_balance_may_go_negative_rather_than_drop_a_real_charge(db: Sessions) -> None:
    """The provider has already billed us by the time we record. Refusing the
    deduction would not un-spend the money, only hide it."""
    _use_test_price()
    user_id = await _make_user(db, "1")
    await record_call_in_session(
        db,
        BillingContext(workspace_id=DEFAULT_WORKSPACE_ID, user_id=user_id),
        model_slug=TEST_MODEL,
        usage=LLMUsage(prompt_tokens=1_000_000, completion_tokens=100_000),
    )
    async with db() as session:
        balance = await credit_balance(session, user_id)
        assert balance is not None and balance < 0
        # ...and the account is then refused new work.
        assert not await has_credit(session, user_id)


# --- the metering wrapper ------------------------------------------------


async def test_the_wrapper_bills_every_call_and_returns_the_response_untouched(
    db: Sessions,
) -> None:
    _use_test_price()
    user_id = await _make_user(db)
    run_id = uuid.uuid4()
    inner = ScriptedLLM(
        [
            LLMResponse(content="one", usage=LLMUsage(prompt_tokens=1000, completion_tokens=100)),
            LLMResponse(content="two", usage=LLMUsage(prompt_tokens=2000, completion_tokens=200)),
        ]
    )
    client = MeteredLLMClient(
        inner, db, BillingContext(workspace_id=DEFAULT_WORKSPACE_ID, user_id=user_id, run_id=run_id)
    )

    first = await client.complete(model=TEST_MODEL, messages=[{"role": "user", "content": "a"}])
    second = await client.complete(model=TEST_MODEL, messages=[{"role": "user", "content": "b"}])

    assert (first.content, second.content) == ("one", "two")
    logs = await _logs_for(db, user_id)
    assert [(log.prompt_tokens, log.completion_tokens) for log in logs] == [
        (1000, 100),
        (2000, 200),
    ]
    async with db() as session:
        assert await run_credits_used(session, run_id) == sum(
            (log.credits_deducted for log in logs), Decimal(0)
        )


async def test_the_charge_is_priced_from_the_requested_slug_not_the_echoed_one(
    db: Sessions,
) -> None:
    """Providers rewrite ``response.model`` (routing, version pinning). The model
    we priced must be the model we audit, or the ledger cannot be reconciled."""
    _use_test_price()
    user_id = await _make_user(db)
    inner = ScriptedLLM(
        [
            LLMResponse(
                content="x",
                model="some-upstream-rewrite-v2",
                usage=LLMUsage(prompt_tokens=1000, completion_tokens=0),
            )
        ]
    )
    client = MeteredLLMClient(
        inner, db, BillingContext(workspace_id=DEFAULT_WORKSPACE_ID, user_id=user_id)
    )
    await client.complete(model=TEST_MODEL, messages=[])
    (log,) = await _logs_for(db, user_id)
    assert log.model_slug == TEST_MODEL


async def test_a_billing_failure_never_fails_the_agents_turn(db: Sessions) -> None:
    """The provider already answered and already charged us; raising here would
    discard completed work over a bookkeeping error."""
    _use_test_price()

    class BrokenSessions:
        def __call__(self, *args: object, **kwargs: object) -> object:
            raise RuntimeError("database is down")

    client = MeteredLLMClient(
        ScriptedLLM(
            [LLMResponse(content="done", usage=LLMUsage(prompt_tokens=10, completion_tokens=1))]
        ),
        BrokenSessions(),  # type: ignore[arg-type]
        BillingContext(workspace_id=DEFAULT_WORKSPACE_ID, user_id=uuid.uuid4()),
    )
    response = await client.complete(model=TEST_MODEL, messages=[])
    assert response.content == "done"


# --- accounts ------------------------------------------------------------


async def test_resolving_a_user_twice_returns_one_account(db: Sessions) -> None:
    """Two concurrent first-time launches must not create two accounts that then
    race each other's balances."""
    email = f"dup-{uuid.uuid4().hex[:8]}@example.com"
    async with session_scope(db) as session:
        first = await resolve_user_id(session, email)
    async with session_scope(db) as session:
        second = await resolve_user_id(session, email.upper())
    assert first == second


async def test_auth_disabled_bills_the_local_account_not_nobody(db: Sessions) -> None:
    """A billing system that is bypassed by turning off auth is not a billing
    system — the metering path must be exercised in dev exactly as in prod."""
    async with session_scope(db) as session:
        user_id = await resolve_user_id(session, None)
        assert await credit_balance(session, user_id) is not None


async def test_granting_credits_raises_the_balance_atomically(db: Sessions) -> None:
    user_id = await _make_user(db, "10")
    async with session_scope(db) as session:
        after = await grant_credits(session, user_id, Decimal("40.5"))
    assert after == Decimal("50.500000")


async def test_an_account_with_no_row_is_not_gated(db: Sessions) -> None:
    async with db() as session:
        assert await has_credit(session, None)
        assert await has_credit(session, uuid.uuid4())
