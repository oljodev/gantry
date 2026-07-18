"""In-process fanout of task-event notifications to WebSocket subscribers.

One dedicated LISTEN connection per server process, however many sockets are
attached. Notifications are *wakeup hints*: a subscriber never receives event
data through the broker — it re-reads ``task_events`` from its own cursor.
That makes delivery loss harmless (the poll fallback catches up) and restarts
trivial (cursors live client-side).
"""

from __future__ import annotations

import asyncio
import contextlib
import json
import uuid
from collections.abc import Iterator
from types import TracebackType
from typing import Self

import asyncpg

from gantry.core.notify import TASK_EVENTS_CHANNEL, asyncpg_dsn
from gantry.logging import get_logger

logger = get_logger(__name__)

#: Subscription key for "every task" (the dashboard firehose).
ALL_TASKS = None


class Subscription:
    """A coalescing wakeup signal: N notifications while busy == one wakeup."""

    def __init__(self) -> None:
        self._wakeup = asyncio.Event()

    def notify(self) -> None:
        self._wakeup.set()

    async def wait(self, timeout_seconds: float) -> bool:
        """True if woken by a notification, False on timeout (poll fallback)."""
        try:
            await asyncio.wait_for(self._wakeup.wait(), timeout_seconds)
        except TimeoutError:
            return False
        self._wakeup.clear()
        return True


class EventBroker:
    def __init__(self, database_url: str) -> None:
        self._dsn = asyncpg_dsn(database_url)
        self._conn: asyncpg.Connection | None = None
        self._subs: dict[uuid.UUID | None, set[Subscription]] = {}

    async def __aenter__(self) -> Self:
        self._conn = await asyncpg.connect(self._dsn)
        await self._conn.add_listener(TASK_EVENTS_CHANNEL, self._on_notify)
        return self

    async def __aexit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        tb: TracebackType | None,
    ) -> None:
        if self._conn is not None:
            with contextlib.suppress(Exception):
                await self._conn.remove_listener(TASK_EVENTS_CHANNEL, self._on_notify)
                await self._conn.close(timeout=5)
            self._conn = None

    @contextlib.contextmanager
    def subscribe(self, task_id: uuid.UUID | None = ALL_TASKS) -> Iterator[Subscription]:
        """Subscribe to one task's events, or to all tasks (``ALL_TASKS``)."""
        sub = Subscription()
        self._subs.setdefault(task_id, set()).add(sub)
        try:
            yield sub
        finally:
            peers = self._subs.get(task_id)
            if peers is not None:
                peers.discard(sub)
                if not peers:
                    del self._subs[task_id]

    def _on_notify(
        self,
        connection: asyncpg.Connection,
        pid: int,
        channel: str,
        payload: object,
    ) -> None:
        try:
            task_id = uuid.UUID(json.loads(str(payload))["task_id"])
        except (ValueError, KeyError, TypeError):  # pragma: no cover - defensive
            logger.warning("broker.bad_notification", payload=str(payload))
            return
        for sub in self._subs.get(task_id, ()):
            sub.notify()
        for sub in self._subs.get(ALL_TASKS, ()):
            sub.notify()
