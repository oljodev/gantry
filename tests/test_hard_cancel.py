"""Operator "Stop all": a live task is interrupted mid-step and lands CANCELLED
in well under a second — not at the next step boundary."""

from __future__ import annotations

import asyncio
from pathlib import Path
from typing import Any

from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.models import DEFAULT_WORKSPACE_ID, TaskKind, TaskStatus
from gantry.core.notify import TASK_CANCEL_CHANNEL, QueueListener
from gantry.worker.service import Worker, WorkerConfig

from .fakes import final_response
from .test_worker_service import enqueue, get_task

Sessions = async_sessionmaker[AsyncSession]


class _BlockingLLM:
    """Simulates a long in-flight LLM call that never returns on its own."""

    async def complete(self, **kwargs: Any) -> Any:
        await asyncio.sleep(60)
        return final_response("done")


async def test_hard_cancel_stops_a_running_agent_under_a_second(
    db: Sessions, tmp_path: Path
) -> None:
    # Co-pilot task => no sandbox, so the run reaches the (blocked) LLM call fast.
    task = await enqueue(db, {"copilot": "tree", "goal": "x"})
    config = WorkerConfig(
        worker_id="hc",
        workspace_root=tmp_path / "ws",
        lease_seconds=30,
        poll_interval_seconds=0.05,
        concurrency=1,
    )
    worker = Worker(db, config, _BlockingLLM())
    shutdown = asyncio.Event()
    run = asyncio.create_task(worker.run(shutdown))
    loop = asyncio.get_running_loop()
    try:
        # Wait until the agent is genuinely running (slot registered, mid-LLM-call).
        deadline = loop.time() + 5
        while task.id not in worker._running:
            assert loop.time() < deadline, "task never started running"
            await asyncio.sleep(0.02)

        t0 = loop.time()
        worker._request_hard_cancel(task.id)  # what the cancel NOTIFY triggers
        while (await get_task(db, task)).status is not TaskStatus.CANCELLED:
            assert loop.time() - t0 < 2, "did not stop promptly"
            await asyncio.sleep(0.02)
        assert loop.time() - t0 < 1.0  # sub-second, mid-step
    finally:
        shutdown.set()
        await run

    final = await get_task(db, task)
    assert final.status is TaskStatus.CANCELLED
    assert final.claimed_by is None  # terminal, not left leased for retry


async def test_cancel_emits_a_cancel_notification(database_url: str, db: Sessions) -> None:
    # A leased task's cancel fires on the cancel channel so its worker wakes now.
    async with session_scope(db) as session:
        task = await queue.enqueue(
            session,
            workspace_id=DEFAULT_WORKSPACE_ID,
            kind=TaskKind.EXECUTE,
            payload={"goal": "x"},
        )
    async with session_scope(db) as session:
        await queue.claim(session, worker_id="w", lease_seconds=30)

    received: list[str] = []
    async with QueueListener(database_url, channel=TASK_CANCEL_CHANNEL, on_payload=received.append):
        await asyncio.sleep(0.05)  # ensure LISTEN is active
        async with session_scope(db) as session:
            result = await queue.cancel(session, task_id=task.id)
        assert result is not None and result.requested
        for _ in range(100):
            if received:
                break
            await asyncio.sleep(0.02)
    assert str(task.id) in received
