"""WebSocket streaming: live projections over the task event log.

Protocol (per-task stream):

- on connect: ``{"type": "task", "data": <snapshot>}`` then every event with
  ``seq > after_seq`` as ``{"type": "event", "data": ...}`` — replay and live
  tail are the same code path, so a client that reconnects with its last seen
  seq never misses or duplicates an event.
- after any lifecycle event (``task_*``): a fresh ``task`` snapshot, so
  clients track status without polling. When the task reaches a terminal
  status the stream closes normally (code 1000).

The broker only ever *wakes* this handler; data always comes from the log.
A jittered poll fallback bounds staleness if a notification is lost.
"""

from __future__ import annotations

import asyncio
import contextlib
import random
import uuid
from collections.abc import Iterator
from typing import cast

import sqlalchemy as sa
from fastapi import APIRouter, WebSocket, WebSocketDisconnect
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core.events import read_events
from gantry.core.models import TERMINAL_STATUSES, Task, TaskEvent
from gantry.server.auth import ws_authenticated
from gantry.server.broker import ALL_TASKS, EventBroker, Subscription
from gantry.server.schemas import EventMessage, TaskEventOut, TaskMessage, TaskOut

router = APIRouter()

POLL_FALLBACK_SECONDS = 5.0
_BATCH = 500

Sessions = async_sessionmaker[AsyncSession]


def _state(websocket: WebSocket) -> tuple[Sessions, EventBroker]:
    app = websocket.app
    return cast("Sessions", app.state.sessions), cast("EventBroker", app.state.broker)


@router.websocket("/api/tasks/{task_id}/events/ws")
async def task_events_ws(websocket: WebSocket, task_id: uuid.UUID, after_seq: int = 0) -> None:
    sessions, broker = _state(websocket)
    await websocket.accept()
    if not await ws_authenticated(websocket):
        return
    task = await _get_task(sessions, task_id)
    if task is None:
        await websocket.close(code=4404, reason="task not found")
        return

    cursor = after_seq
    with broker.subscribe(task_id) as sub, _disconnect_watch(websocket, sub) as gone:
        try:
            await websocket.send_text(
                TaskMessage(data=TaskOut.model_validate(task)).model_dump_json()
            )
            while not gone.is_set():
                cursor, saw_lifecycle = await _send_task_events(
                    websocket, sessions, task_id, cursor
                )
                if saw_lifecycle:
                    task = await _get_task(sessions, task_id)
                    if task is None:  # pragma: no cover - tasks are never deleted mid-stream
                        break
                    await websocket.send_text(
                        TaskMessage(data=TaskOut.model_validate(task)).model_dump_json()
                    )
                    if task.status in TERMINAL_STATUSES:
                        await websocket.close(code=1000)
                        return
                await sub.wait(_jittered(POLL_FALLBACK_SECONDS))
        except WebSocketDisconnect:
            pass


@router.websocket("/api/events/ws")
async def firehose_ws(websocket: WebSocket, after_id: int | None = None) -> None:
    """All tasks' events, cursored by the global ``task_events.id``.

    A monitoring surface (the dashboard), not a replay surface: under
    concurrent commits a lower id can become visible after the cursor passed
    it, so per-task correctness always comes from the per-task stream's dense
    ``seq``. ``after_id`` omitted means "tail from now".
    """
    sessions, broker = _state(websocket)
    await websocket.accept()
    if not await ws_authenticated(websocket):
        return
    cursor = after_id if after_id is not None else await _max_event_id(sessions)
    with broker.subscribe(ALL_TASKS) as sub, _disconnect_watch(websocket, sub) as gone:
        try:
            while not gone.is_set():
                cursor = await _send_all_events(websocket, sessions, cursor)
                await sub.wait(_jittered(POLL_FALLBACK_SECONDS))
        except WebSocketDisconnect:
            pass


async def _get_task(sessions: Sessions, task_id: uuid.UUID) -> Task | None:
    async with sessions() as session:
        return await session.get(Task, task_id)


async def _max_event_id(sessions: Sessions) -> int:
    async with sessions() as session:
        value = await session.scalar(sa.select(sa.func.max(TaskEvent.id)))
    return value or 0


async def _send_task_events(
    websocket: WebSocket, sessions: Sessions, task_id: uuid.UUID, cursor: int
) -> tuple[int, bool]:
    """Send everything after ``cursor``; returns (new cursor, saw task_* event)."""
    saw_lifecycle = False
    while True:
        async with sessions() as session:
            events = await read_events(session, task_id, after_seq=cursor, limit=_BATCH)
        for event in events:
            await websocket.send_text(
                EventMessage(data=TaskEventOut.model_validate(event)).model_dump_json()
            )
            cursor = event.seq
            saw_lifecycle = saw_lifecycle or event.event_type.value.startswith("task_")
        if len(events) < _BATCH:
            return cursor, saw_lifecycle


async def _send_all_events(websocket: WebSocket, sessions: Sessions, cursor: int) -> int:
    while True:
        async with sessions() as session:
            events = (
                await session.scalars(
                    sa.select(TaskEvent)
                    .where(TaskEvent.id > cursor)
                    .order_by(TaskEvent.id)
                    .limit(_BATCH)
                )
            ).all()
        for event in events:
            await websocket.send_text(
                EventMessage(data=TaskEventOut.model_validate(event)).model_dump_json()
            )
            cursor = event.id
        if len(events) < _BATCH:
            return cursor


def _jittered(seconds: float) -> float:
    return seconds * random.uniform(0.8, 1.2)


@contextlib.contextmanager
def _disconnect_watch(websocket: WebSocket, sub: Subscription) -> Iterator[asyncio.Event]:
    """Notice client disconnects while we are stream-only (we never read data).

    Sets the returned event and pokes the subscription so a handler dozing in
    ``sub.wait`` exits immediately instead of at the next poll tick.
    """
    gone = asyncio.Event()

    async def _reader() -> None:
        with contextlib.suppress(WebSocketDisconnect, RuntimeError):
            while True:
                await websocket.receive_text()
        gone.set()
        sub.notify()

    reader = asyncio.create_task(_reader())
    try:
        yield gone
    finally:
        reader.cancel()
