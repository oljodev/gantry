from __future__ import annotations

import asyncio
import uuid
from typing import Any

import sqlalchemy as sa
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.events import read_events
from gantry.core.models import (
    DEFAULT_WORKSPACE_ID,
    EventType,
    Task,
    TaskKind,
    TaskStatus,
)

Sessions = async_sessionmaker[AsyncSession]


async def enqueue_one(
    db: Sessions,
    *,
    priority: int = 0,
    max_attempts: int = 3,
    payload: dict[str, Any] | None = None,
) -> Task:
    async with session_scope(db) as session:
        return await queue.enqueue(
            session,
            workspace_id=DEFAULT_WORKSPACE_ID,
            kind=TaskKind.EXECUTE,
            payload=payload or {},
            priority=priority,
            max_attempts=max_attempts,
        )


async def get_task(db: Sessions, task_id: uuid.UUID) -> Task:
    async with session_scope(db) as session:
        task = await session.get(Task, task_id)
        assert task is not None
        return task


async def test_enqueue_sets_root_and_logs_event(db: Sessions) -> None:
    task = await enqueue_one(db, payload={"goal": "x"})
    assert task.root_task_id == task.id
    assert task.status is TaskStatus.PENDING

    async with session_scope(db) as session:
        child = await queue.enqueue(
            session,
            workspace_id=DEFAULT_WORKSPACE_ID,
            kind=TaskKind.EXECUTE,
            payload={},
            parent=task,
        )
        assert child.parent_task_id == task.id
        assert child.root_task_id == task.id

    async with session_scope(db) as session:
        events = await read_events(session, task.id)
    assert [e.event_type for e in events] == [EventType.TASK_ENQUEUED]
    assert events[0].seq == 1


async def test_claim_empty_queue_returns_none(db: Sessions) -> None:
    async with session_scope(db) as session:
        assert await queue.claim(session, worker_id="w1") is None


async def test_claim_takes_highest_priority_first(db: Sessions) -> None:
    low = await enqueue_one(db, priority=0)
    high = await enqueue_one(db, priority=10)

    async with session_scope(db) as session:
        first = await queue.claim(session, worker_id="w1")
    async with session_scope(db) as session:
        second = await queue.claim(session, worker_id="w1")

    assert first is not None and first.id == high.id
    assert second is not None and second.id == low.id
    assert first.status is TaskStatus.CLAIMED
    assert first.attempt == 1
    assert first.claimed_by == "w1"
    assert first.lease_expires_at is not None


async def test_future_scheduled_task_is_not_claimable(db: Sessions) -> None:
    task = await enqueue_one(db)
    async with session_scope(db) as session:
        await session.execute(
            sa.update(Task)
            .where(Task.id == task.id)
            .values(scheduled_at=sa.func.now() + sa.text("interval '1 hour'"))
        )
    async with session_scope(db) as session:
        assert await queue.claim(session, worker_id="w1") is None


async def test_concurrent_claimers_never_get_the_same_task(db: Sessions) -> None:
    for _ in range(2):
        await enqueue_one(db)

    # Hold the first claim's transaction open while a second claimer runs:
    # SKIP LOCKED must route it to the other task, without blocking.
    async with db() as session_a, db() as session_b:
        task_a = await queue.claim(session_a, worker_id="a")  # uncommitted
        task_b = await asyncio.wait_for(queue.claim(session_b, worker_id="b"), timeout=5)
        await session_a.commit()
        await session_b.commit()

    assert task_a is not None and task_b is not None
    assert task_a.id != task_b.id


async def test_complete_is_exactly_once(db: Sessions) -> None:
    task = await enqueue_one(db)
    async with session_scope(db) as session:
        claimed = await queue.claim(session, worker_id="w1")
    assert claimed is not None

    async with session_scope(db) as session:
        ok = await queue.complete(
            session, task_id=task.id, worker_id="w1", attempt=1, result={"answer": 42}
        )
    assert ok

    async with session_scope(db) as session:
        again = await queue.complete(session, task_id=task.id, worker_id="w1", attempt=1)
    assert not again

    final = await get_task(db, task.id)
    assert final.status is TaskStatus.SUCCEEDED
    assert final.result == {"answer": 42}
    assert final.claimed_by is None and final.lease_expires_at is None


async def test_fail_requeues_with_backoff_then_fails_terminally(db: Sessions) -> None:
    task = await enqueue_one(db, max_attempts=2)

    async with session_scope(db) as session:
        claimed = await queue.claim(session, worker_id="w1")
    assert claimed is not None and claimed.attempt == 1
    async with session_scope(db) as session:
        status = await queue.fail(session, task_id=task.id, worker_id="w1", attempt=1, error="boom")
    assert status is TaskStatus.PENDING

    refreshed = await get_task(db, task.id)
    assert refreshed.last_error == "boom"
    # Backoff pushed scheduled_at into the future — not claimable yet.
    async with session_scope(db) as session:
        assert await queue.claim(session, worker_id="w2") is None

    # Make it due now, claim as attempt 2, and fail again → terminal.
    async with session_scope(db) as session:
        await session.execute(
            sa.update(Task).where(Task.id == task.id).values(scheduled_at=sa.func.now())
        )
    async with session_scope(db) as session:
        reclaimed = await queue.claim(session, worker_id="w2")
    assert reclaimed is not None and reclaimed.attempt == 2
    async with session_scope(db) as session:
        status = await queue.fail(
            session, task_id=task.id, worker_id="w2", attempt=2, error="boom again"
        )
    assert status is TaskStatus.FAILED

    final = await get_task(db, task.id)
    assert final.status is TaskStatus.FAILED

    async with session_scope(db) as session:
        events = await read_events(session, task.id)
    assert [e.event_type for e in events] == [
        EventType.TASK_ENQUEUED,
        EventType.TASK_CLAIMED,
        EventType.TASK_RETRY_SCHEDULED,
        EventType.TASK_CLAIMED,
        EventType.TASK_FAILED,
    ]


async def test_reaper_requeues_expired_lease(db: Sessions) -> None:
    task = await enqueue_one(db)
    async with session_scope(db) as session:
        claimed = await queue.claim(session, worker_id="dying", lease_seconds=0.05)
    assert claimed is not None
    await asyncio.sleep(0.1)

    async with session_scope(db) as session:
        reaped = await queue.reap_expired(session)
    assert [r.task_id for r in reaped] == [task.id]
    assert reaped[0].status is TaskStatus.PENDING

    refreshed = await get_task(db, task.id)
    assert refreshed.status is TaskStatus.PENDING
    assert refreshed.claimed_by is None
    assert refreshed.attempt == 1  # attempt is consumed; next claim is attempt 2


async def test_reaper_fails_task_out_of_attempts(db: Sessions) -> None:
    task = await enqueue_one(db, max_attempts=1)
    async with session_scope(db) as session:
        await queue.claim(session, worker_id="dying", lease_seconds=0.05)
    await asyncio.sleep(0.1)

    async with session_scope(db) as session:
        reaped = await queue.reap_expired(session)
    assert reaped[0].status is TaskStatus.FAILED

    final = await get_task(db, task.id)
    assert final.status is TaskStatus.FAILED
    assert final.last_error is not None


async def test_reaper_ignores_live_leases(db: Sessions) -> None:
    await enqueue_one(db)
    async with session_scope(db) as session:
        await queue.claim(session, worker_id="alive", lease_seconds=60)
    async with session_scope(db) as session:
        assert await queue.reap_expired(session) == []


async def test_zombie_worker_is_fenced_after_reap(db: Sessions) -> None:
    """The attempt fencing token: a reaped-and-reclaimed task rejects its old owner."""
    task = await enqueue_one(db)
    async with session_scope(db) as session:
        zombie = await queue.claim(session, worker_id="zombie", lease_seconds=0.05)
    assert zombie is not None and zombie.attempt == 1
    await asyncio.sleep(0.1)

    async with session_scope(db) as session:
        await queue.reap_expired(session)
    async with session_scope(db) as session:
        fresh = await queue.claim(session, worker_id="fresh")
    assert fresh is not None and fresh.attempt == 2

    # The zombie wakes up and tries to act on its stale claim — all rejected.
    async with session_scope(db) as session:
        assert not await queue.heartbeat(session, task_id=task.id, worker_id="zombie", attempt=1)
        assert not await queue.complete(session, task_id=task.id, worker_id="zombie", attempt=1)
        assert (
            await queue.fail(session, task_id=task.id, worker_id="zombie", attempt=1, error="x")
        ) is None

    # The rightful owner still works.
    async with session_scope(db) as session:
        assert await queue.heartbeat(session, task_id=task.id, worker_id="fresh", attempt=2)
        assert await queue.complete(session, task_id=task.id, worker_id="fresh", attempt=2)


async def test_heartbeat_extends_lease(db: Sessions) -> None:
    task = await enqueue_one(db)
    async with session_scope(db) as session:
        claimed = await queue.claim(session, worker_id="w1", lease_seconds=1)
    assert claimed is not None
    before = claimed.lease_expires_at
    assert before is not None

    async with session_scope(db) as session:
        assert await queue.heartbeat(
            session, task_id=task.id, worker_id="w1", attempt=1, lease_seconds=120
        )
    refreshed = await get_task(db, task.id)
    assert refreshed.lease_expires_at is not None
    assert refreshed.lease_expires_at > before


def test_retry_backoff_is_exponential_and_capped() -> None:
    assert queue.retry_backoff_seconds(1, base=5, cap=300) == 5
    assert queue.retry_backoff_seconds(2, base=5, cap=300) == 10
    assert queue.retry_backoff_seconds(3, base=5, cap=300) == 20
    assert queue.retry_backoff_seconds(10, base=5, cap=300) == 300
