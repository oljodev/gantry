from __future__ import annotations

import asyncio

from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core import queue
from gantry.core.models import DEFAULT_WORKSPACE_ID, TaskKind
from gantry.core.notify import QueueListener

from .test_queue import enqueue_one

Sessions = async_sessionmaker[AsyncSession]


async def test_wait_times_out_when_nothing_happens(database_url: str) -> None:
    async with QueueListener(database_url) as listener:
        assert await listener.wait(timeout_seconds=0.1) is False


async def test_enqueue_wakes_listener(database_url: str, db: Sessions) -> None:
    async with QueueListener(database_url) as listener:
        waiter = asyncio.create_task(listener.wait(timeout_seconds=5))
        await asyncio.sleep(0.05)  # ensure the waiter is listening first
        await enqueue_one(db)
        assert await waiter is True


async def test_rolled_back_enqueue_does_not_wake_listener(database_url: str, db: Sessions) -> None:
    """NOTIFY is transactional: a rollback must not produce a wakeup."""
    async with QueueListener(database_url) as listener:
        async with db() as session:
            await queue.enqueue(
                session,
                workspace_id=DEFAULT_WORKSPACE_ID,
                kind=TaskKind.EXECUTE,
                payload={},
            )
            await session.rollback()
        assert await listener.wait(timeout_seconds=0.3) is False
