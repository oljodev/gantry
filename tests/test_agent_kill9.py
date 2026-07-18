"""Phase 2 acceptance: an agent is kill -9'd mid-task; a fresh process
finishes it correctly from the event log.

Proven against real Postgres:
- the first worker dies mid-run and the task is NOT completed by it
- the reaper re-queues the orphaned task after lease expiry
- a second, fresh process rehydrates and continues (resumed=True)
- exactly `TARGET` successful tool results exist in the log — the resumed
  run did not redo checkpointed steps
"""

from __future__ import annotations

import asyncio
import os
import signal
import subprocess
import sys
from pathlib import Path

import pytest
import sqlalchemy as sa
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.models import DEFAULT_WORKSPACE_ID, Task, TaskKind, TaskStatus

REPO_ROOT = Path(__file__).resolve().parent.parent
TARGET = 8
LEASE_SECONDS = 3.0
WORK_SECONDS = 0.15

Sessions = async_sessionmaker[AsyncSession]


def spawn_runner(database_url: str, worker_id: str, effect_file: Path) -> subprocess.Popen[bytes]:
    return subprocess.Popen(
        [sys.executable, "-m", "tests.agent_runner", worker_id],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "GANTRY_DATABASE_URL": database_url,
            "AGENT_LEASE_SECONDS": str(LEASE_SECONDS),
            "AGENT_TARGET": str(TARGET),
            "AGENT_EFFECT_FILE": str(effect_file),
            "AGENT_WORK_SECONDS": str(WORK_SECONDS),
        },
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )


async def get_task(db: Sessions, task_id: object) -> Task:
    async with session_scope(db) as session:
        task = await session.get(Task, task_id)
        assert task is not None
        return task


@pytest.mark.slow
async def test_agent_survives_kill9_and_finishes_from_log(
    database_url: str, db: Sessions, tmp_path: Path
) -> None:
    effect_file = tmp_path / "increments.log"

    async with session_scope(db) as session:
        task = await queue.enqueue(
            session,
            workspace_id=DEFAULT_WORKSPACE_ID,
            kind=TaskKind.EXECUTE,
            payload={"goal": f"count to {TARGET}", "model": "fake/counting"},
            max_attempts=5,
        )

    # First worker: wait until the side-effect log proves it is mid-run
    # (a couple of the 8 increments done), then kill -9 mid-flight.
    first = spawn_runner(database_url, "agent-1", effect_file)
    kill_deadline = asyncio.get_running_loop().time() + 20
    while True:
        done = len(effect_file.read_text().splitlines()) if effect_file.exists() else 0
        if done >= 2:
            break
        assert first.poll() is None, "first worker exited before doing any work"
        assert asyncio.get_running_loop().time() < kill_deadline, "worker never made progress"
        await asyncio.sleep(0.05)
    assert first.poll() is None, "first worker finished before we could kill it"
    first.send_signal(signal.SIGKILL)
    first.wait(timeout=10)

    interrupted = await get_task(db, task.id)
    assert interrupted.status in (TaskStatus.CLAIMED, TaskStatus.RUNNING)
    partial = effect_file.read_text().splitlines() if effect_file.exists() else []
    assert 0 < len(partial) < TARGET, f"kill landed outside the work window: {partial}"

    # The reaper notices the expired lease and re-queues the task.
    deadline = asyncio.get_running_loop().time() + LEASE_SECONDS + 10
    while True:
        async with session_scope(db) as session:
            await queue.reap_expired(session)
        current = await get_task(db, task.id)
        if current.status is TaskStatus.PENDING:
            break
        assert asyncio.get_running_loop().time() < deadline, "task never reaped"
        await asyncio.sleep(0.2)

    # A fresh process picks it up and finishes from the log.
    second = spawn_runner(database_url, "agent-2", effect_file)
    assert second.wait(timeout=60) == 0

    final = await get_task(db, task.id)
    assert final.status is TaskStatus.SUCCEEDED
    assert final.result is not None
    assert final.result["final_text"] == f"done after {TARGET} increments"
    assert final.result["resumed"] is True

    async with session_scope(db) as session:
        ok_results = await session.scalar(
            sa.text(
                """
                SELECT count(*) FROM task_events
                WHERE event_type = 'tool_result' AND (payload->>'is_error')::bool = false
                """
            )
        )
        succeeded_events = await session.scalar(
            sa.text("SELECT count(*) FROM task_events WHERE event_type = 'task_succeeded'")
        )
    # Checkpointed steps were not redone: exactly TARGET durable results...
    assert ok_results == TARGET
    assert succeeded_events == 1
    # ...though raw side effects may exceed TARGET by at most the one
    # in-flight step (at-least-once execution of the interrupted call).
    effects = effect_file.read_text().splitlines()
    assert TARGET <= len(effects) <= TARGET + 1
