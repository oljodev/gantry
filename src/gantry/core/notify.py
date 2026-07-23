"""Postgres LISTEN/NOTIFY wakeups for idle workers.

NOTIFY issued inside a transaction is delivered only on commit — exactly the
semantics we want (workers must not wake for tasks that were rolled back).
Notifications are best-effort (a disconnected listener misses them), so
workers always keep a jittered poll as fallback; NOTIFY only shortcuts the
latency between enqueue and pickup.
"""

from __future__ import annotations

import asyncio
import contextlib
import json
import uuid
from collections.abc import Callable
from types import TracebackType
from typing import Self

import asyncpg
import sqlalchemy as sa
from sqlalchemy.ext.asyncio import AsyncSession

TASK_READY_CHANNEL = "gantry_task_ready"
#: Fires once per appended task event — the control plane LISTENs here and
#: fans out to WebSocket subscribers. The payload is a hint, not the data:
#: subscribers always re-read the event log from their cursor.
TASK_EVENTS_CHANNEL = "gantry_task_events"
#: Fires when an operator cancels a live task — carries the task id so a worker
#: holding that task can interrupt it immediately, instead of waiting for its
#: next heartbeat poll and step boundary.
TASK_CANCEL_CHANNEL = "gantry_task_cancel"


def asyncpg_dsn(database_url: str) -> str:
    """SQLAlchemy URLs carry a '+asyncpg' driver marker that asyncpg itself rejects."""
    return database_url.replace("postgresql+asyncpg://", "postgresql://", 1)


async def notify_task_ready(session: AsyncSession, task_id: uuid.UUID) -> None:
    await session.execute(sa.select(sa.func.pg_notify(TASK_READY_CHANNEL, str(task_id))))


async def notify_task_event(session: AsyncSession, task_id: uuid.UUID, seq: int) -> None:
    payload = json.dumps({"task_id": str(task_id), "seq": seq})
    await session.execute(sa.select(sa.func.pg_notify(TASK_EVENTS_CHANNEL, payload)))


async def notify_task_cancel(session: AsyncSession, task_id: uuid.UUID) -> None:
    """Signal (on commit) that a live task should stop now — the worker holding
    it interrupts the in-flight step instead of waiting for its heartbeat poll."""
    await session.execute(sa.select(sa.func.pg_notify(TASK_CANCEL_CHANNEL, str(task_id))))


class QueueListener:
    """LISTENs on the task-ready channel; workers await wakeups with a timeout.

    Uses a dedicated asyncpg connection: LISTEN is connection-scoped state,
    which pooled sessions can't provide reliably.
    """

    def __init__(
        self,
        database_url: str,
        channel: str = TASK_READY_CHANNEL,
        *,
        on_payload: Callable[[str], None] | None = None,
    ) -> None:
        self._dsn = asyncpg_dsn(database_url)
        self._channel = channel
        self._conn: asyncpg.Connection | None = None
        self._wakeup = asyncio.Event()
        #: Optional per-notification hook (e.g. a worker cancelling a slot). Runs
        #: on the event loop before the coalesced wakeup is set. Assignable after
        #: construction so a listener can be wired to its owner once it exists.
        self.on_payload = on_payload

    async def __aenter__(self) -> Self:
        self._conn = await asyncpg.connect(self._dsn)
        await self._conn.add_listener(self._channel, self._on_notify)
        return self

    async def __aexit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        tb: TracebackType | None,
    ) -> None:
        if self._conn is not None:
            with contextlib.suppress(Exception):
                await self._conn.remove_listener(self._channel, self._on_notify)
                await self._conn.close(timeout=5)
            self._conn = None

    def _on_notify(
        self,
        connection: asyncpg.Connection,
        pid: int,
        channel: str,
        payload: object,
    ) -> None:
        if self.on_payload is not None:
            with contextlib.suppress(Exception):
                self.on_payload(str(payload))
        self._wakeup.set()

    async def wait(self, timeout_seconds: float) -> bool:
        """Wait for a wakeup or timeout. True if a notification arrived.

        Coalesces bursts: N notifications while idle yield one wakeup, which
        is fine — a woken worker claims in a loop until the queue is empty.
        """
        try:
            await asyncio.wait_for(self._wakeup.wait(), timeout_seconds)
        except TimeoutError:
            return False
        self._wakeup.clear()
        return True
