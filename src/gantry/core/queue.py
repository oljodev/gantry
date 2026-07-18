"""Durable task queue operations.

Concurrency model:

- **Claiming** is a single statement: a CTE selects the next runnable task
  with ``FOR UPDATE SKIP LOCKED`` and the outer UPDATE claims it. Thousands
  of workers can poll concurrently without collisions or lock waits.
- **Leases, not held locks.** A claim grants a lease (``lease_expires_at``);
  workers heartbeat to extend it. The reaper re-queues tasks whose lease
  expired (worker died) — combined with event-log replay, a killed worker
  loses only its in-flight step.
- **Fencing.** ``attempt`` increments atomically at claim time and every
  worker mutation must present (worker_id, attempt). A zombie worker whose
  task was reaped and re-claimed can no longer complete/fail/heartbeat it.
- All timestamps come from the database clock (``now()``), never the host's.
"""

from __future__ import annotations

import uuid
from dataclasses import dataclass
from datetime import datetime
from typing import Any, cast

import sqlalchemy as sa
from sqlalchemy.ext.asyncio import AsyncSession

from gantry.core.events import append_event
from gantry.core.models import (
    LEASED_STATUSES,
    EventType,
    Task,
    TaskKind,
    TaskStatus,
)
from gantry.core.notify import notify_task_ready
from gantry.logging import get_logger

logger = get_logger(__name__)

DEFAULT_LEASE_SECONDS = 60.0
DEFAULT_RETRY_BACKOFF_BASE_SECONDS = 5.0
DEFAULT_RETRY_BACKOFF_CAP_SECONDS = 300.0


def _rowcount(result: sa.Result[Any]) -> int:
    return cast("sa.CursorResult[Any]", result).rowcount


def _now_plus(seconds: float) -> sa.ColumnElement[datetime]:
    return sa.func.now() + sa.func.make_interval(0, 0, 0, 0, 0, 0, seconds)


def retry_backoff_seconds(
    attempt: int,
    base: float = DEFAULT_RETRY_BACKOFF_BASE_SECONDS,
    cap: float = DEFAULT_RETRY_BACKOFF_CAP_SECONDS,
) -> float:
    """Exponential backoff for retry N (attempt is 1-based): base * 2^(n-1), capped."""
    return min(base * float(2 ** max(attempt - 1, 0)), cap)


@dataclass(frozen=True)
class ReapedTask:
    task_id: uuid.UUID
    status: TaskStatus
    attempt: int
    claimed_by: str | None


async def enqueue(
    session: AsyncSession,
    *,
    workspace_id: uuid.UUID,
    kind: TaskKind,
    payload: dict[str, Any],
    parent: Task | None = None,
    priority: int = 0,
    max_attempts: int = 3,
    scheduled_at: datetime | None = None,
) -> Task:
    """Insert a pending task, log it, and notify idle workers (on commit)."""
    task_id = uuid.uuid4()
    task = Task(
        id=task_id,
        workspace_id=workspace_id,
        parent_task_id=parent.id if parent else None,
        root_task_id=parent.root_task_id if parent else task_id,
        kind=kind,
        payload=payload,
        priority=priority,
        max_attempts=max_attempts,
    )
    if scheduled_at is not None:
        task.scheduled_at = scheduled_at
    session.add(task)
    await session.flush()
    await append_event(
        session,
        task.id,
        EventType.TASK_ENQUEUED,
        {"kind": kind.value, "priority": priority, "parent_task_id": _opt(task.parent_task_id)},
    )
    await notify_task_ready(session, task.id)
    return task


async def claim(
    session: AsyncSession,
    *,
    worker_id: str,
    lease_seconds: float = DEFAULT_LEASE_SECONDS,
) -> Task | None:
    """Atomically claim the next runnable task, or return None if queue is empty.

    Highest priority first, then earliest scheduled_at (so retried tasks
    re-enter fairly). SKIP LOCKED means concurrent claimers never wait on or
    receive the same row.
    """
    next_task = (
        sa.select(Task.id)
        .where(Task.status == TaskStatus.PENDING, Task.scheduled_at <= sa.func.now())
        .order_by(Task.priority.desc(), Task.scheduled_at)
        .limit(1)
        .with_for_update(skip_locked=True)
        .cte("next_task")
    )
    stmt = (
        sa.update(Task)
        .where(Task.id == sa.select(next_task.c.id).scalar_subquery())
        .values(
            status=TaskStatus.CLAIMED,
            claimed_by=worker_id,
            attempt=Task.attempt + 1,
            lease_expires_at=_now_plus(lease_seconds),
            updated_at=sa.func.now(),
        )
        .returning(Task)
    )
    task = (await session.scalars(stmt)).first()
    if task is None:
        return None
    await append_event(
        session,
        task.id,
        EventType.TASK_CLAIMED,
        {"worker_id": worker_id, "attempt": task.attempt},
    )
    return task


async def heartbeat(
    session: AsyncSession,
    *,
    task_id: uuid.UUID,
    worker_id: str,
    attempt: int,
    lease_seconds: float = DEFAULT_LEASE_SECONDS,
) -> bool:
    """Extend the lease. False means the lease was lost — the worker must abort."""
    result = await session.execute(
        sa.update(Task)
        .where(
            Task.id == task_id,
            Task.claimed_by == worker_id,
            Task.attempt == attempt,
            Task.status.in_(LEASED_STATUSES),
        )
        .values(lease_expires_at=_now_plus(lease_seconds), updated_at=sa.func.now())
    )
    return _rowcount(result) == 1


async def mark_running(
    session: AsyncSession,
    *,
    task_id: uuid.UUID,
    worker_id: str,
    attempt: int,
) -> bool:
    result = await session.execute(
        sa.update(Task)
        .where(
            Task.id == task_id,
            Task.claimed_by == worker_id,
            Task.attempt == attempt,
            Task.status == TaskStatus.CLAIMED,
        )
        .values(status=TaskStatus.RUNNING, updated_at=sa.func.now())
    )
    return _rowcount(result) == 1


async def complete(
    session: AsyncSession,
    *,
    task_id: uuid.UUID,
    worker_id: str,
    attempt: int,
    result: dict[str, Any] | None = None,
) -> bool:
    """Mark succeeded. Transactional with its event — completion is exactly-once."""
    res = await session.execute(
        sa.update(Task)
        .where(
            Task.id == task_id,
            Task.claimed_by == worker_id,
            Task.attempt == attempt,
            Task.status.in_(LEASED_STATUSES),
        )
        .values(
            status=TaskStatus.SUCCEEDED,
            result=result,
            claimed_by=None,
            lease_expires_at=None,
            updated_at=sa.func.now(),
        )
    )
    if _rowcount(res) != 1:
        return False
    await append_event(
        session,
        task_id,
        EventType.TASK_SUCCEEDED,
        {"worker_id": worker_id, "attempt": attempt},
    )
    return True


async def fail(
    session: AsyncSession,
    *,
    task_id: uuid.UUID,
    worker_id: str,
    attempt: int,
    error: str,
    retryable: bool = True,
    backoff_base_seconds: float = DEFAULT_RETRY_BACKOFF_BASE_SECONDS,
    backoff_cap_seconds: float = DEFAULT_RETRY_BACKOFF_CAP_SECONDS,
) -> TaskStatus | None:
    """Record a failure: re-queue with backoff, or fail terminally.

    Returns the resulting status, or None if the fencing check rejected the
    write (lease lost — some other attempt owns the task now).
    """
    guard = (
        Task.id == task_id,
        Task.claimed_by == worker_id,
        Task.attempt == attempt,
        Task.status.in_(LEASED_STATUSES),
    )
    will_retry = retryable and attempt < await _max_attempts_of(session, task_id)
    if will_retry:
        delay = retry_backoff_seconds(attempt, backoff_base_seconds, backoff_cap_seconds)
        res = await session.execute(
            sa.update(Task)
            .where(*guard)
            .values(
                status=TaskStatus.PENDING,
                last_error=error,
                claimed_by=None,
                lease_expires_at=None,
                scheduled_at=_now_plus(delay),
                updated_at=sa.func.now(),
            )
        )
        if _rowcount(res) != 1:
            return None
        await append_event(
            session,
            task_id,
            EventType.TASK_RETRY_SCHEDULED,
            {"worker_id": worker_id, "attempt": attempt, "error": error, "delay_seconds": delay},
        )
        return TaskStatus.PENDING

    res = await session.execute(
        sa.update(Task)
        .where(*guard)
        .values(
            status=TaskStatus.FAILED,
            last_error=error,
            claimed_by=None,
            lease_expires_at=None,
            updated_at=sa.func.now(),
        )
    )
    if _rowcount(res) != 1:
        return None
    await append_event(
        session,
        task_id,
        EventType.TASK_FAILED,
        {"worker_id": worker_id, "attempt": attempt, "error": error},
    )
    return TaskStatus.FAILED


async def cancel(session: AsyncSession, *, task_id: uuid.UUID) -> Task | None:
    """Cancel a task that has not started yet (compare-and-set on PENDING).

    Running tasks are owned by a lease-holding worker; cancelling those needs
    worker cooperation (checked at heartbeat) and lands with HITL in Phase 7.
    Returns the cancelled task, or None if it wasn't pending.
    """
    stmt = (
        sa.update(Task)
        .where(Task.id == task_id, Task.status == TaskStatus.PENDING)
        .values(
            status=TaskStatus.CANCELLED,
            claimed_by=None,
            lease_expires_at=None,
            updated_at=sa.func.now(),
        )
        .returning(Task)
    )
    task = (await session.scalars(stmt)).first()
    if task is None:
        return None
    await append_event(session, task.id, EventType.TASK_CANCELLED, {})
    return task


async def reap_expired(session: AsyncSession, *, limit: int = 100) -> list[ReapedTask]:
    """Re-queue (or terminally fail) tasks whose lease expired.

    SKIP LOCKED keeps the reaper from ever blocking on a live worker's row;
    a worker that is merely slow will simply heartbeat and move on.
    """
    expired = (
        sa.select(Task.id)
        .where(Task.status.in_(LEASED_STATUSES), Task.lease_expires_at < sa.func.now())
        .limit(limit)
        .with_for_update(skip_locked=True)
        .cte("expired_tasks")
    )
    stmt = (
        sa.update(Task)
        .where(Task.id.in_(sa.select(expired.c.id)))
        .values(
            status=sa.case(
                (Task.attempt >= Task.max_attempts, TaskStatus.FAILED.value),
                else_=TaskStatus.PENDING.value,
            ),
            last_error=sa.case(
                (
                    Task.attempt >= Task.max_attempts,
                    "lease expired on final attempt (worker died?)",
                ),
                else_=Task.last_error,
            ),
            claimed_by=None,
            lease_expires_at=None,
            updated_at=sa.func.now(),
        )
        .returning(Task.id, Task.status, Task.attempt, Task.claimed_by)
    )
    rows = (await session.execute(stmt)).all()
    reaped: list[ReapedTask] = []
    for row in rows:
        item = ReapedTask(
            task_id=row.id, status=TaskStatus(row.status), attempt=row.attempt, claimed_by=None
        )
        reaped.append(item)
        await append_event(
            session,
            item.task_id,
            EventType.TASK_LEASE_EXPIRED,
            {"attempt": item.attempt, "requeued": item.status is TaskStatus.PENDING},
        )
        if item.status is TaskStatus.PENDING:
            await notify_task_ready(session, item.task_id)
        logger.info(
            "queue.task_reaped",
            task_id=str(item.task_id),
            attempt=item.attempt,
            new_status=item.status,
        )
    return reaped


async def _max_attempts_of(session: AsyncSession, task_id: uuid.UUID) -> int:
    value = await session.scalar(sa.select(Task.max_attempts).where(Task.id == task_id))
    return value if value is not None else 0


def _opt(value: uuid.UUID | None) -> str | None:
    return str(value) if value is not None else None
