"""Append-only event store for task execution traces.

``seq`` is allocated as ``max(seq) + 1`` inside the INSERT itself. Under
normal operation only the current lease holder writes a task's events, so
collisions are rare; when two writers do race (e.g. reaper vs. zombie
worker), the ``UNIQUE (task_id, seq)`` constraint rejects one and we retry.
"""

from __future__ import annotations

import asyncio
import random
import uuid
from typing import Any

import sqlalchemy as sa
from sqlalchemy.dialects.postgresql import JSONB, UUID
from sqlalchemy.exc import IntegrityError
from sqlalchemy.ext.asyncio import AsyncSession

from gantry.core.models import EventType, TaskEvent

# A burst of N racing writers can force the unluckiest one through N-1
# collisions (one winner per round), so this bounds burst size, not luck.
_MAX_SEQ_RETRIES = 32


class EventSeqConflictError(RuntimeError):
    """Raised when seq allocation keeps colliding — indicates a hot writer race."""


async def append_event(
    session: AsyncSession,
    task_id: uuid.UUID,
    event_type: EventType,
    payload: dict[str, Any] | None = None,
) -> int:
    """Append one event; returns its seq.

    Uses a nested transaction (SAVEPOINT) per try so a unique-violation
    rollback does not poison the caller's outer transaction.
    """
    for attempt_no in range(_MAX_SEQ_RETRIES):
        if attempt_no:
            await asyncio.sleep(random.uniform(0, 0.01) * attempt_no)  # decorrelate racers
        try:
            async with session.begin_nested():
                seq = await session.scalar(
                    sa.insert(TaskEvent)
                    .from_select(
                        ["task_id", "seq", "event_type", "payload"],
                        sa.select(
                            sa.literal(task_id, UUID(as_uuid=True)),
                            sa.func.coalesce(sa.func.max(TaskEvent.seq), 0) + 1,
                            sa.literal(event_type.value, sa.String(32)),
                            sa.literal(payload or {}, JSONB()),
                        ).where(TaskEvent.task_id == task_id),
                    )
                    .returning(TaskEvent.seq)
                )
        except IntegrityError:
            continue
        if seq is None:  # pragma: no cover - from_select over WHERE always yields one row
            raise EventSeqConflictError(f"event insert produced no row for task {task_id}")
        return seq
    raise EventSeqConflictError(f"could not allocate event seq for task {task_id}")


async def read_events(
    session: AsyncSession,
    task_id: uuid.UUID,
    after_seq: int = 0,
) -> list[TaskEvent]:
    """Read a task's events in replay order, optionally after a known seq."""
    result = await session.scalars(
        sa.select(TaskEvent)
        .where(TaskEvent.task_id == task_id, TaskEvent.seq > after_seq)
        .order_by(TaskEvent.seq)
    )
    return list(result)
