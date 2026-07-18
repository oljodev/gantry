"""Standalone agent worker process for the kill -9 acceptance test.

Claims one task from the queue, runs the durable agent loop with a
deterministic fake LLM (CountingToolLLM) and a file-side-effect tool, and
completes the task. The parent test SIGKILLs the first instance mid-run; a
second instance must finish the task from the event log.

Run as: python -m tests.agent_runner <worker_id>
"""

from __future__ import annotations

import asyncio
import os
import sys
from pathlib import Path

from sqlalchemy.ext.asyncio import async_sessionmaker, create_async_engine
from sqlalchemy.pool import NullPool

from gantry.core import queue
from gantry.core.db import session_scope
from gantry.runtime.loop import run_agent_task
from gantry.runtime.tools import ToolRegistry

from .fakes import CountingToolLLM, FileIncrementTool


async def main(worker_id: str) -> int:
    database_url = os.environ["GANTRY_DATABASE_URL"]
    lease_seconds = float(os.environ.get("AGENT_LEASE_SECONDS", "3"))
    target = int(os.environ.get("AGENT_TARGET", "8"))
    effect_file = Path(os.environ["AGENT_EFFECT_FILE"])
    work_seconds = float(os.environ.get("AGENT_WORK_SECONDS", "0.15"))

    engine = create_async_engine(database_url, poolclass=NullPool)
    factory = async_sessionmaker(engine, expire_on_commit=False)

    task = None
    for _ in range(50):
        async with session_scope(factory) as session:
            task = await queue.claim(session, worker_id=worker_id, lease_seconds=lease_seconds)
        if task is not None:
            break
        await asyncio.sleep(0.1)
    if task is None:
        return 3  # nothing to do

    async def heartbeat() -> None:
        async with session_scope(factory) as session:
            alive = await queue.heartbeat(
                session,
                task_id=task.id,
                worker_id=worker_id,
                attempt=task.attempt,
                lease_seconds=lease_seconds,
            )
        if not alive:
            raise RuntimeError("lease lost — another worker owns this task now")

    outcome = await run_agent_task(
        factory,
        task,
        CountingToolLLM(target),
        ToolRegistry([FileIncrementTool(effect_file, work_seconds=work_seconds)]),
        on_step=heartbeat,
    )

    async with session_scope(factory) as session:
        completed = await queue.complete(
            session,
            task_id=task.id,
            worker_id=worker_id,
            attempt=task.attempt,
            result={
                "final_text": outcome.final_text,
                "steps": outcome.steps,
                "resumed": outcome.resumed,
            },
        )
    await engine.dispose()
    return 0 if completed else 4


if __name__ == "__main__":
    sys.exit(asyncio.run(main(sys.argv[1])))
