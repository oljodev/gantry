from __future__ import annotations

import asyncio

from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core.db import session_scope
from gantry.core.events import append_event, read_events
from gantry.core.models import EventType

from .test_queue import enqueue_one

Sessions = async_sessionmaker[AsyncSession]


async def test_seq_is_monotonic_from_one(db: Sessions) -> None:
    task = await enqueue_one(db)  # writes seq 1 (task_enqueued)
    async with session_scope(db) as session:
        for i in range(2, 6):
            seq = await append_event(session, task.id, EventType.TERMINAL_CHUNK, {"i": i})
            assert seq == i

    async with session_scope(db) as session:
        events = await read_events(session, task.id)
    assert [e.seq for e in events] == [1, 2, 3, 4, 5]


async def test_concurrent_appenders_get_unique_seqs(db: Sessions) -> None:
    """Racing writers on separate connections must serialize via the unique
    constraint + retry, never lose or duplicate a seq."""
    task = await enqueue_one(db)

    async def append_one(i: int) -> int:
        async with session_scope(db) as session:
            return await append_event(session, task.id, EventType.TERMINAL_CHUNK, {"i": i})

    seqs = await asyncio.gather(*(append_one(i) for i in range(10)))
    assert sorted(seqs) == list(range(2, 12))


async def test_read_events_after_seq(db: Sessions) -> None:
    task = await enqueue_one(db)
    async with session_scope(db) as session:
        await append_event(session, task.id, EventType.TERMINAL_CHUNK, {"n": 1})
        await append_event(session, task.id, EventType.TERMINAL_CHUNK, {"n": 2})

    async with session_scope(db) as session:
        tail = await read_events(session, task.id, after_seq=2)
    assert [e.payload["n"] for e in tail] == [2]
