"""Phase 1 acceptance: 50 worker processes vs 1,000 tasks, with kill -9 chaos.

Proves, against a real Postgres:
- zero double-claims (no two workers ever hold the same (task, attempt))
- zero lost tasks (every task reaches SUCCEEDED despite SIGKILLed workers)
- exactly-once completion (completion is transactional with its event)
- the reaper actually re-queues tasks orphaned by dead workers
"""

from __future__ import annotations

import asyncio
import os
import random
import signal
import subprocess
import sys
from pathlib import Path

import pytest
import sqlalchemy as sa
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.models import DEFAULT_WORKSPACE_ID, Task, TaskKind

N_TASKS = 1000
N_WORKERS = 50
N_KILLS = 10
LEASE_SECONDS = 2.0
REAP_INTERVAL = 0.25
TIMEOUT_SECONDS = 180

WORKER_SCRIPT = Path(__file__).with_name("torture_worker.py")

Sessions = async_sessionmaker[AsyncSession]


def spawn_worker(database_url: str, index: int) -> subprocess.Popen[bytes]:
    return subprocess.Popen(
        [sys.executable, str(WORKER_SCRIPT), f"torture-{index}"],
        env={
            **os.environ,
            "GANTRY_DATABASE_URL": database_url,
            "TORTURE_LEASE_SECONDS": str(LEASE_SECONDS),
        },
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )


async def count_by_status(db: Sessions) -> dict[str, int]:
    async with session_scope(db) as session:
        rows = (
            await session.execute(sa.select(Task.status, sa.func.count()).group_by(Task.status))
        ).all()
    return {str(status): count for status, count in rows}


@pytest.mark.slow
async def test_torture_50_workers_1000_tasks_with_kill9(database_url: str, db: Sessions) -> None:
    # -- Arrange: 1,000 tasks -------------------------------------------------
    async with session_scope(db) as session:
        for i in range(N_TASKS):
            await queue.enqueue(
                session,
                workspace_id=DEFAULT_WORKSPACE_ID,
                kind=TaskKind.EXECUTE,
                payload={"n": i},
                max_attempts=10,  # chaos may burn several attempts per task
            )

    workers = [spawn_worker(database_url, i) for i in range(N_WORKERS)]
    reaper_stop = asyncio.Event()

    async def reaper_loop() -> None:
        while not reaper_stop.is_set():
            async with session_scope(db) as session:
                await queue.reap_expired(session, limit=200)
            await asyncio.sleep(REAP_INTERVAL)

    reaper = asyncio.create_task(reaper_loop())

    try:
        # -- Act: kill 10 workers while the queue is in full swing ------------
        deadline = asyncio.get_running_loop().time() + TIMEOUT_SECONDS
        killed = 0
        while True:
            counts = await count_by_status(db)
            succeeded = counts.get("succeeded", 0)

            if killed < N_KILLS and succeeded > 100:
                victim = random.choice([w for w in workers if w.poll() is None])
                victim.send_signal(signal.SIGKILL)
                victim.wait(timeout=10)
                killed += 1

            remaining = sum(counts.get(s, 0) for s in ("pending", "claimed", "running"))
            if remaining == 0 and succeeded + counts.get("failed", 0) == N_TASKS:
                break
            if asyncio.get_running_loop().time() > deadline:
                pytest.fail(f"torture run timed out; status counts: {counts}")
            await asyncio.sleep(0.2)
    finally:
        reaper_stop.set()
        await reaper
        for w in workers:
            if w.poll() is None:
                w.terminate()
        for w in workers:
            if w.poll() is None:
                try:
                    w.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    w.kill()

    # -- Assert ---------------------------------------------------------------
    counts = await count_by_status(db)
    assert counts.get("succeeded") == N_TASKS, f"lost tasks! status counts: {counts}"

    async with session_scope(db) as session:
        # Exactly-once completion: precisely one task_succeeded event per task.
        bad_completions = (
            await session.execute(
                sa.text(
                    """
                    SELECT task_id, count(*) FROM task_events
                    WHERE event_type = 'task_succeeded'
                    GROUP BY task_id HAVING count(*) <> 1
                    """
                )
            )
        ).all()
        assert bad_completions == [], f"non-exactly-once completions: {bad_completions[:5]}"

        succeeded_events = await session.scalar(
            sa.text("SELECT count(*) FROM task_events WHERE event_type = 'task_succeeded'")
        )
        assert succeeded_events == N_TASKS

        # Zero double-claims: each (task, attempt) was claimed at most once.
        double_claims = (
            await session.execute(
                sa.text(
                    """
                    SELECT task_id, payload->>'attempt', count(*) FROM task_events
                    WHERE event_type = 'task_claimed'
                    GROUP BY task_id, payload->>'attempt' HAVING count(*) > 1
                    """
                )
            )
        ).all()
        assert double_claims == [], f"double claims detected: {double_claims[:5]}"

        # The chaos was real: dead workers' leases expired and were reaped,
        # and at least one task needed more than one attempt.
        reaped_count = await session.scalar(
            sa.text("SELECT count(*) FROM task_events WHERE event_type = 'task_lease_expired'")
        )
        assert reaped_count is not None and reaped_count >= 1
        max_attempt = await session.scalar(sa.select(sa.func.max(Task.attempt)))
        assert max_attempt is not None and max_attempt > 1

    assert killed == N_KILLS
