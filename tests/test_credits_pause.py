"""Graceful pause on zero balance, and a clean resume once credits arrive.

The behaviour these lock down is the difference between a billing limit that is
usable and one that is not: running out of money must stop the swarm at a step
boundary, keep everything it has done, and pick up exactly where it stopped —
never crash, never lose the log, never half-run a tool call.
"""

from __future__ import annotations

import uuid
from decimal import Decimal
from pathlib import Path

import httpx
import sqlalchemy as sa
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.billing.catalog import CATALOG, TokenPrice
from gantry.billing.gate import CreditGate
from gantry.billing.users import grant_credits, resolve_user_id
from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.events import read_events
from gantry.core.models import (
    DEFAULT_WORKSPACE_ID,
    EventType,
    Task,
    TaskKind,
    TaskStatus,
    User,
)
from gantry.runtime.llm import LLMResponse
from gantry.worker.service import Worker, WorkerConfig

from .fakes import ScriptedLLM, final_response, response_with_tool_call
from .test_worker_service import claim_as, get_task

Sessions = async_sessionmaker[AsyncSession]

TEST_MODEL = "fake/test"
#: Priced so a single scripted response (10 prompt / 5 completion tokens) costs
#: well over one credit, so "spend until empty" needs no long loops.
EXPENSIVE = TokenPrice(Decimal(100_000), Decimal(300_000), Decimal(10_000))


def _tree_call(call_id: str) -> LLMResponse:
    """A SUCCEEDING co-pilot tool call, so the step after it is a plain step
    boundary — an erroring tool would trip the completion guard instead and
    confuse what the pause is being attributed to."""
    return response_with_tool_call(
        call_id, "propose_tree", {"name": "Team", "root": {"name": "root"}}
    )


def _worker(db: Sessions, tmp_path: Path, llm: ScriptedLLM, **overrides: object) -> Worker:
    config = WorkerConfig(
        worker_id="svc-worker-1",
        workspace_root=tmp_path / "workspaces",
        poll_interval_seconds=0.05,
        enforce_credit_balance=True,
        **overrides,  # type: ignore[arg-type]
    )
    return Worker(db, config, llm)


async def _owned_task(
    db: Sessions, credits: str, payload: dict[str, object] | None = None
) -> tuple[Task, uuid.UUID]:
    async with session_scope(db) as session:
        user_id = await resolve_user_id(session, f"pause-{uuid.uuid4().hex[:6]}@example.com")
        await session.execute(
            sa.update(User)
            .where(User.id == user_id)
            .values(gantry_credits_balance=Decimal(credits))
        )
        task = await queue.enqueue(
            session,
            workspace_id=DEFAULT_WORKSPACE_ID,
            kind=TaskKind.EXECUTE,
            # Co-pilot tasks skip the sandbox and git, isolating the pause path.
            payload={"model": TEST_MODEL, "copilot": "tree", "goal": "design", **(payload or {})},
            user_id=user_id,
        )
    return task, user_id


async def _balance(db: Sessions, user_id: uuid.UUID) -> Decimal:
    async with db() as session:
        value = await session.scalar(
            sa.select(User.gantry_credits_balance).where(User.id == user_id)
        )
    assert value is not None
    return Decimal(value)


# --- the gate itself -----------------------------------------------------


def test_the_gate_trips_only_on_an_observed_non_positive_balance() -> None:
    gate = CreditGate(enabled=True, balance=Decimal(10))
    assert not gate.exhausted
    gate.observe(Decimal("0.5"))
    assert not gate.exhausted
    gate.observe(Decimal(0))
    assert gate.exhausted


def test_an_unknown_balance_never_reads_as_out_of_money() -> None:
    """ "We don't know" must not pause a run — an unowned call reports None, and
    treating that as zero would strand work nobody is even billing."""
    gate = CreditGate(enabled=True, balance=Decimal(5))
    gate.observe(None)
    assert gate.balance == Decimal(5)
    assert not gate.exhausted
    assert not CreditGate(enabled=True).exhausted


def test_a_disabled_gate_never_trips() -> None:
    gate = CreditGate(enabled=False, balance=Decimal(-100))
    assert not gate.exhausted


# --- pausing -------------------------------------------------------------


async def test_a_run_that_hits_zero_pauses_instead_of_failing(db: Sessions, tmp_path: Path) -> None:
    CATALOG.load({TEST_MODEL: EXPENSIVE})
    task, _ = await _owned_task(db, credits="1")
    # The first call drains the balance; the pause lands at the NEXT step
    # boundary, so the script must offer a step after it.
    worker = _worker(db, tmp_path, ScriptedLLM([_tree_call("call-1"), final_response("done")]))

    await worker.process(await claim_as(db, worker))

    settled = await get_task(db, task)
    assert settled.status is TaskStatus.PAUSED_OUT_OF_CREDITS
    assert settled.last_error is None
    # Parked means unleased — the slot is released, not held hostage.
    assert settled.claimed_by is None
    assert settled.lease_expires_at is None


async def test_the_pause_lands_at_a_step_boundary_with_the_tool_call_finished(
    db: Sessions, tmp_path: Path
) -> None:
    """A tool call already in flight must complete cleanly; the run stops
    BETWEEN actions, never inside one — a half-applied edit is unrecoverable."""
    CATALOG.load({TEST_MODEL: EXPENSIVE})
    task, _ = await _owned_task(db, credits="1", payload={"copilot": "tree"})
    worker = _worker(
        db,
        tmp_path,
        ScriptedLLM(
            [
                response_with_tool_call("call-1", "propose_tree", {"tree": {"name": "root"}}),
                final_response("never reached"),
            ]
        ),
    )

    await worker.process(await claim_as(db, worker))

    assert (await get_task(db, task)).status is TaskStatus.PAUSED_OUT_OF_CREDITS
    async with db() as session:
        events = await read_events(session, task.id)
    kinds = [e.event_type for e in events]
    # The tool call that was already running produced its result before the pause.
    assert EventType.TOOL_RESULT.value in kinds
    assert kinds[-1] == EventType.TASK_PARKED.value
    parked = events[-1]
    assert parked.payload["reason"] == TaskStatus.PAUSED_OUT_OF_CREDITS.value


async def test_the_pause_records_the_balance_that_caused_it(db: Sessions, tmp_path: Path) -> None:
    CATALOG.load({TEST_MODEL: EXPENSIVE})
    task, _ = await _owned_task(db, credits="1")
    worker = _worker(db, tmp_path, ScriptedLLM([_tree_call("call-1"), final_response("done")]))
    await worker.process(await claim_as(db, worker))

    async with db() as session:
        events = await read_events(session, task.id)
    parked = next(e for e in events if e.event_type == EventType.TASK_PARKED.value)
    # The figure shown to the user, recorded as it was when the call was made.
    assert Decimal(parked.payload["balance"]) <= 0


async def test_an_unowned_task_is_never_paused(db: Sessions, tmp_path: Path) -> None:
    """Work enqueued outside the API has no balance to run out of; gating it
    would strand runs nobody is billing."""
    CATALOG.load({TEST_MODEL: EXPENSIVE})
    async with session_scope(db) as session:
        task = await queue.enqueue(
            session,
            workspace_id=DEFAULT_WORKSPACE_ID,
            kind=TaskKind.EXECUTE,
            payload={"model": TEST_MODEL, "copilot": "tree", "goal": "unowned"},
        )
    worker = _worker(db, tmp_path, ScriptedLLM([final_response("ran")]))
    await worker.process(await claim_as(db, worker))
    assert (await get_task(db, task)).status is TaskStatus.SUCCEEDED


# --- resuming ------------------------------------------------------------


async def test_a_topped_up_run_resumes_and_finishes_from_its_log(
    db: Sessions, tmp_path: Path
) -> None:
    """The whole point: the paused run continues from where it stopped rather
    than restarting, so nothing already paid for is paid for twice."""
    CATALOG.load({TEST_MODEL: EXPENSIVE})
    task, user_id = await _owned_task(db, credits="1")
    # First attempt: one tool call lands, then the balance is gone and it pauses.
    first = _worker(
        db,
        tmp_path,
        ScriptedLLM([_tree_call("call-1")]),
    )
    await first.process(await claim_as(db, first))
    assert (await get_task(db, task)).status is TaskStatus.PAUSED_OUT_OF_CREDITS

    # Top up, which wakes the run.
    async with session_scope(db) as session:
        await grant_credits(session, user_id, Decimal("100000"))
        woken = await queue.resume_paused_accounts(session, user_id=user_id)
    assert woken == 1
    assert (await get_task(db, task)).status is TaskStatus.PENDING

    # The resumed attempt continues the SAME conversation: it is handed ONLY the
    # finishing response and still succeeds, which it could not do if it had
    # replayed from step one (it would run out of scripted responses).
    second = _worker(db, tmp_path, ScriptedLLM([final_response("finished after resume")]))
    await second.process(await claim_as(db, second))

    settled = await get_task(db, task)
    assert settled.status is TaskStatus.SUCCEEDED
    assert settled.result is not None
    assert "finished after resume" in str(settled.result.get("final_text", ""))


async def test_resuming_an_unfunded_run_is_refused_rather_than_churning(
    db: Sessions, tmp_path: Path
) -> None:
    """Waking a still-broke run would re-pause it at the next step, burn an
    attempt, and fill the trace with churn that reads as a failed resume."""
    CATALOG.load({TEST_MODEL: EXPENSIVE})
    task, _ = await _owned_task(db, credits="1")
    worker = _worker(db, tmp_path, ScriptedLLM([_tree_call("call-1"), final_response("done")]))
    await worker.process(await claim_as(db, worker))
    assert (await get_task(db, task)).status is TaskStatus.PAUSED_OUT_OF_CREDITS

    async with session_scope(db) as session:
        woken, funded = await queue.resume_paused_run(session, root_task_id=task.root_task_id)
    assert (woken, funded) == (0, False)
    assert (await get_task(db, task)).status is TaskStatus.PAUSED_OUT_OF_CREDITS


async def test_a_top_up_that_lands_mid_park_does_not_strand_the_run(
    db: Sessions, tmp_path: Path
) -> None:
    """The lost-wakeup race: credits arriving between the worker deciding to
    pause and the park committing must not leave a funded run parked forever.
    The park re-reads the balance in its own transaction and re-queues itself."""
    task, user_id = await _owned_task(db, credits="0")
    async with session_scope(db) as session:
        claimed = await queue.claim(session, worker_id="race-worker")
    assert claimed is not None

    # The top-up commits BEFORE the park runs — exactly the window that would
    # otherwise strand the task.
    async with session_scope(db) as session:
        await grant_credits(session, user_id, Decimal("500"))
    async with session_scope(db) as session:
        status = await queue.park_for_credits(
            session, task_id=task.id, worker_id="race-worker", attempt=claimed.attempt
        )
    assert status is TaskStatus.PENDING
    assert (await get_task(db, task)).status is TaskStatus.PENDING


async def test_a_whole_swarm_wakes_together(db: Sessions, tmp_path: Path) -> None:
    """A leader and its children pause within moments of each other; waking them
    one at a time would let an early leader survey a tree that is mid-resume."""
    task, user_id = await _owned_task(db, credits="0")
    async with session_scope(db) as session:
        parent = await session.get(Task, task.id)
        assert parent is not None
        children = [
            await queue.enqueue(
                session,
                workspace_id=DEFAULT_WORKSPACE_ID,
                kind=TaskKind.EXECUTE,
                payload={"goal": f"child {i}"},
                parent=parent,
            )
            for i in range(3)
        ]
        await session.execute(
            sa.update(Task)
            .where(Task.id.in_([task.id, *(c.id for c in children)]))
            .values(status=TaskStatus.PAUSED_OUT_OF_CREDITS)
        )
        await grant_credits(session, user_id, Decimal("1000"))

    async with session_scope(db) as session:
        woken, funded = await queue.resume_paused_run(session, root_task_id=task.root_task_id)
    assert (woken, funded) == (4, True)
    async with db() as session:
        statuses = (
            await session.scalars(
                sa.select(Task.status).where(Task.root_task_id == task.root_task_id)
            )
        ).all()
    assert all(s is TaskStatus.PENDING for s in statuses)


async def test_a_resume_lifts_max_attempts_so_pausing_does_not_erode_retries(
    db: Sessions, tmp_path: Path
) -> None:
    """A pause is not a failure, so it must not consume the task's error-retry
    budget — otherwise a run that paused a few times would fail for lack of
    attempts having never actually errored."""
    task, user_id = await _owned_task(db, credits="0")
    async with session_scope(db) as session:
        await session.execute(
            sa.update(Task)
            .where(Task.id == task.id)
            .values(status=TaskStatus.PAUSED_OUT_OF_CREDITS, attempt=3, max_attempts=3)
        )
        await grant_credits(session, user_id, Decimal(1000))
    async with session_scope(db) as session:
        await queue.resume_paused_run(session, root_task_id=task.root_task_id)

    resumed = await get_task(db, task)
    assert resumed.status is TaskStatus.PENDING
    assert resumed.max_attempts > resumed.attempt


# --- the API surface -----------------------------------------------------


async def test_the_resume_endpoint_reports_what_it_woke(
    db: Sessions, client: httpx.AsyncClient
) -> None:
    created = await client.post("/api/tasks", json={"goal": "will pause"})
    root_id = uuid.UUID(created.json()["id"])
    async with session_scope(db) as session:
        await session.execute(
            sa.update(Task)
            .where(Task.id == root_id)
            .values(status=TaskStatus.PAUSED_OUT_OF_CREDITS)
        )

    response = await client.post(f"/api/credits/runs/{root_id}/resume")
    assert response.status_code == 200, response.text
    assert response.json() == {"run_id": str(root_id), "resumed_tasks": 1}
    async with db() as session:
        task = await session.get(Task, root_id)
        assert task is not None and task.status is TaskStatus.PENDING


async def test_the_resume_endpoint_refuses_while_the_balance_is_empty(
    db: Sessions, client: httpx.AsyncClient
) -> None:
    created = await client.post("/api/tasks", json={"goal": "still broke"})
    root_id = uuid.UUID(created.json()["id"])
    user_id = uuid.UUID((await client.get("/api/credits")).json()["user_id"])
    async with session_scope(db) as session:
        await session.execute(
            sa.update(Task)
            .where(Task.id == root_id)
            .values(status=TaskStatus.PAUSED_OUT_OF_CREDITS)
        )
        await session.execute(
            sa.update(User).where(User.id == user_id).values(gantry_credits_balance=Decimal(0))
        )

    response = await client.post(f"/api/credits/runs/{root_id}/resume")
    assert response.status_code == 409
    assert "top up" in response.text.lower()


async def test_a_grant_wakes_the_accounts_paused_runs(
    db: Sessions, client: httpx.AsyncClient
) -> None:
    """Money arrives for the ACCOUNT, so everything it paused comes back — a
    balance that lands without resuming the work it was bought for leaves the
    user staring at a funded, idle swarm."""
    created = await client.post("/api/tasks", json={"goal": "paused"})
    root_id = uuid.UUID(created.json()["id"])
    user_id = uuid.UUID((await client.get("/api/credits")).json()["user_id"])
    async with session_scope(db) as session:
        await session.execute(
            sa.update(Task)
            .where(Task.id == root_id)
            .values(status=TaskStatus.PAUSED_OUT_OF_CREDITS)
        )
        await session.execute(
            sa.update(User).where(User.id == user_id).values(gantry_credits_balance=Decimal(0))
        )

    granted = await client.post("/api/credits/grant", json={"credits": 500})
    assert granted.status_code == 201

    async with db() as session:
        task = await session.get(Task, root_id)
        assert task is not None and task.status is TaskStatus.PENDING
