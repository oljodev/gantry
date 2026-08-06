"""The worker actually meters what it runs.

The ledger unit tests prove the wrapper bills correctly; this proves the wrapper
is really in the path — a real Worker claiming and running a real task must leave
usage rows behind and draw the owner's balance down. Wiring that quietly stops
being applied is exactly the regression a unit test cannot catch.
"""

from __future__ import annotations

import uuid
from decimal import Decimal
from pathlib import Path

import sqlalchemy as sa
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.billing.catalog import CATALOG, TokenPrice
from gantry.billing.ledger import credit_balance
from gantry.billing.users import resolve_user_id
from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.models import DEFAULT_WORKSPACE_ID, LlmUsageLog, Task, TaskKind, TaskStatus
from gantry.worker.service import Worker, WorkerConfig

from .fakes import ScriptedLLM, final_response
from .test_worker_service import claim_as, get_task

Sessions = async_sessionmaker[AsyncSession]

TEST_MODEL = "fake/test"


def _worker(db: Sessions, tmp_path: Path, llm: ScriptedLLM, **overrides: object) -> Worker:
    config = WorkerConfig(
        # Shared with the claim helper below, which claims under this id.
        worker_id="svc-worker-1",
        workspace_root=tmp_path / "workspaces",
        poll_interval_seconds=0.05,
        **overrides,  # type: ignore[arg-type]
    )
    return Worker(db, config, llm)


async def _owned_task(db: Sessions, credits: str = "1000") -> tuple[Task, uuid.UUID]:
    async with session_scope(db) as session:
        user_id = await resolve_user_id(session, f"owner-{uuid.uuid4().hex[:6]}@example.com")
        await session.execute(
            sa.text("UPDATE users SET gantry_credits_balance = :c WHERE id = :i").bindparams(
                c=Decimal(credits), i=user_id
            )
        )
        task = await queue.enqueue(
            session,
            workspace_id=DEFAULT_WORKSPACE_ID,
            kind=TaskKind.EXECUTE,
            # A co-pilot task skips the sandbox and git, isolating the billing path.
            payload={"model": TEST_MODEL, "copilot": "tree", "goal": "design a team"},
            user_id=user_id,
        )
    return task, user_id


async def test_a_run_leaves_a_usage_row_and_draws_the_balance_down(
    db: Sessions, tmp_path: Path
) -> None:
    CATALOG.load({TEST_MODEL: TokenPrice(Decimal(1000), Decimal(3000), Decimal(100))})
    task, user_id = await _owned_task(db)
    worker = _worker(db, tmp_path, ScriptedLLM([final_response("a team design")]))

    await worker.process(await claim_as(db, worker))

    assert (await get_task(db, task)).status is TaskStatus.SUCCEEDED
    async with db() as session:
        rows = list(
            (
                await session.scalars(sa.select(LlmUsageLog).where(LlmUsageLog.user_id == user_id))
            ).all()
        )
        balance = await credit_balance(session, user_id)

    assert len(rows) == 1
    row = rows[0]
    assert row.model_slug == TEST_MODEL
    assert row.task_id == task.id
    # The run id is the ROOT task, so a whole swarm accumulates against one total.
    assert row.run_id == task.root_task_id
    assert (row.prompt_tokens, row.completion_tokens) == (10, 5)
    assert row.credits_deducted > 0
    assert balance == Decimal(1000) - row.credits_deducted


async def test_an_out_of_credit_account_is_stopped_before_spending_anything(
    db: Sessions, tmp_path: Path
) -> None:
    """The gate is BEFORE the first call: a check that ran after a response could
    only record the overrun, never prevent it. And it PAUSES rather than fails —
    running out of money is a billing condition the user can fix, not an error in
    the work (see tests/test_credits_pause.py for the full pause/resume cycle)."""
    CATALOG.load({TEST_MODEL: TokenPrice(Decimal(1000), Decimal(3000), Decimal(100))})
    task, user_id = await _owned_task(db, credits="0")
    llm = ScriptedLLM([final_response("should never run")])
    worker = _worker(db, tmp_path, llm, enforce_credit_balance=True)

    await worker.process(await claim_as(db, worker))

    settled = await get_task(db, task)
    assert settled.status is TaskStatus.PAUSED_OUT_OF_CREDITS
    assert settled.last_error is None  # a pause is not a failure to debug
    # Nothing was sent to the provider, so nothing was charged.
    assert llm.calls == []
    async with db() as session:
        assert (
            await session.scalar(
                sa.select(sa.func.count())
                .select_from(LlmUsageLog)
                .where(LlmUsageLog.user_id == user_id)
            )
        ) == 0


async def test_enforcement_can_be_turned_off_to_track_without_gating(
    db: Sessions, tmp_path: Path
) -> None:
    """A deployment that wants metering but no enforcement lets the balance go
    negative and stay visible instead of pausing."""
    CATALOG.load({TEST_MODEL: TokenPrice(Decimal(1000), Decimal(3000), Decimal(100))})
    task, user_id = await _owned_task(db, credits="0")
    worker = _worker(db, tmp_path, ScriptedLLM([final_response("ran anyway")]))

    await worker.process(await claim_as(db, worker))

    assert (await get_task(db, task)).status is TaskStatus.SUCCEEDED
    async with db() as session:
        balance = await credit_balance(session, user_id)
    assert balance is not None and balance < 0
