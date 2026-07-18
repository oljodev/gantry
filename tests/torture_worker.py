"""Standalone worker process for the queue torture test.

Runs a claim → simulated-work → complete loop forever; the parent test
SIGKILLs a subset of these mid-flight to prove that leases + the reaper +
attempt fencing deliver exactly-once completion. The sleep between claim and
complete is the deliberate kill window.
"""

from __future__ import annotations

import asyncio
import os
import random
import sys

from sqlalchemy.ext.asyncio import async_sessionmaker, create_async_engine
from sqlalchemy.pool import NullPool

from gantry.core import queue
from gantry.core.db import session_scope


async def main(worker_id: str) -> None:
    database_url = os.environ["GANTRY_DATABASE_URL"]
    lease_seconds = float(os.environ.get("TORTURE_LEASE_SECONDS", "2"))
    work_seconds = float(os.environ.get("TORTURE_WORK_SECONDS", "0.02"))

    engine = create_async_engine(database_url, poolclass=NullPool)
    factory = async_sessionmaker(engine, expire_on_commit=False)

    while True:
        async with session_scope(factory) as session:
            task = await queue.claim(session, worker_id=worker_id, lease_seconds=lease_seconds)

        if task is None:
            await asyncio.sleep(random.uniform(0.02, 0.08))
            continue

        await asyncio.sleep(work_seconds)  # ← the kill window

        async with session_scope(factory) as session:
            await queue.complete(
                session,
                task_id=task.id,
                worker_id=worker_id,
                attempt=task.attempt,
                result={"worker": worker_id, "attempt": task.attempt},
            )


if __name__ == "__main__":
    asyncio.run(main(sys.argv[1]))
