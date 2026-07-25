"""The workspace kill switch: stop the whole swarm without chasing task ids.

Per-task cancellation answers "stop this run". This answers the question an
operator actually has when spend runs away — "stop everything, now" — and it has
to hold for workers that were already running, workers that boot afterwards, and
workers whose NOTIFY listener is dead.
"""

from __future__ import annotations

import asyncio
import uuid
from dataclasses import replace
from pathlib import Path
from typing import Any

import httpx
import sqlalchemy as sa
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core import queue
from gantry.core.control import get_control, set_emergency_stop
from gantry.core.db import session_scope
from gantry.core.models import DEFAULT_WORKSPACE_ID, Task, TaskEvent, TaskKind, TaskStatus
from gantry.worker.service import Worker, WorkerConfig

from .fakes import final_response

Sessions = async_sessionmaker[AsyncSession]


class _SlowLLM:
    """A long in-flight call, so a task is genuinely mid-step when we stop it."""

    async def complete(self, **kwargs: Any) -> Any:
        await asyncio.sleep(60)
        return final_response("done")


async def _enqueue(db: Sessions, goal: str) -> Task:
    async with session_scope(db) as session:
        return await queue.enqueue(
            session,
            workspace_id=DEFAULT_WORKSPACE_ID,
            kind=TaskKind.EXECUTE,
            payload={"copilot": "tree", "goal": goal},  # copilot => no sandbox, starts fast
        )


async def _status(db: Sessions, task_id: uuid.UUID) -> TaskStatus:
    async with session_scope(db) as session:
        return TaskStatus(await session.scalar(sa.select(Task.status).where(Task.id == task_id)))


def _config(tmp_path: Path, **kw: Any) -> WorkerConfig:
    return WorkerConfig(
        worker_id=kw.pop("worker_id", "stop-w"),
        workspace_root=tmp_path / "ws",
        lease_seconds=30,
        poll_interval_seconds=0.05,
        control_refresh_seconds=0.05,
        concurrency=4,
        **kw,
    )


# --- the control row ------------------------------------------------------


async def test_absent_row_means_running(db: Sessions) -> None:
    """A workspace that has never been stopped must not need a row to work."""
    async with session_scope(db) as session:
        state = await get_control(session, DEFAULT_WORKSPACE_ID)
    assert state.running and not state.stopped


async def test_trip_and_clear_round_trip(db: Sessions) -> None:
    async with session_scope(db) as session:
        tripped = await set_emergency_stop(
            session, workspace_id=DEFAULT_WORKSPACE_ID, stopped=True, reason="spend spike"
        )
    assert tripped.stopped and tripped.reason == "spend spike"

    async with session_scope(db) as session:
        assert (await get_control(session, DEFAULT_WORKSPACE_ID)).stopped

    async with session_scope(db) as session:
        cleared = await set_emergency_stop(
            session, workspace_id=DEFAULT_WORKSPACE_ID, stopped=False
        )
    assert cleared.running

    async with session_scope(db) as session:
        assert (await get_control(session, DEFAULT_WORKSPACE_ID)).running


async def test_tripping_twice_is_idempotent(db: Sessions) -> None:
    """The upsert must not explode on a second trip — an operator hammering the
    button during an incident is the expected case, not an edge case."""
    for _ in range(3):
        async with session_scope(db) as session:
            await set_emergency_stop(
                session, workspace_id=DEFAULT_WORKSPACE_ID, stopped=True, reason="again"
            )
    async with session_scope(db) as session:
        assert (await get_control(session, DEFAULT_WORKSPACE_ID)).stopped


# --- the dispatcher gate --------------------------------------------------


async def test_a_stopped_workspace_claims_nothing(db: Sessions, tmp_path: Path) -> None:
    """The core guarantee: with the switch tripped, queued work stays queued."""
    async with session_scope(db) as session:
        await set_emergency_stop(
            session, workspace_id=DEFAULT_WORKSPACE_ID, stopped=True, reason="halt"
        )
    task = await _enqueue(db, "must not run")

    worker = Worker(db, _config(tmp_path), _SlowLLM())
    shutdown = asyncio.Event()
    run = asyncio.ensure_future(worker.run(shutdown))
    try:
        await asyncio.sleep(0.6)  # ample time to claim if the gate were open
        assert await _status(db, task.id) is TaskStatus.PENDING
        assert worker.processed == 0
        assert not worker._running
    finally:
        shutdown.set()
        await run


async def test_tripping_mid_run_halts_in_flight_agents(db: Sessions, tmp_path: Path) -> None:
    """Draining — letting in-flight agents finish — would keep burning exactly
    the spend the switch exists to stop, so running tasks are halted too."""
    tasks = [await _enqueue(db, f"running {i}") for i in range(3)]

    worker = Worker(db, _config(tmp_path), _SlowLLM())
    shutdown = asyncio.Event()
    run = asyncio.ensure_future(worker.run(shutdown))
    loop = asyncio.get_running_loop()
    try:
        deadline = loop.time() + 10
        while len(worker._running) < 3:
            assert loop.time() < deadline, "agents never started"
            await asyncio.sleep(0.02)

        t0 = loop.time()
        async with session_scope(db) as session:
            await set_emergency_stop(
                session, workspace_id=DEFAULT_WORKSPACE_ID, stopped=True, reason="runaway"
            )

        for task in tasks:
            while await _status(db, task.id) is not TaskStatus.CANCELLED:
                assert loop.time() - t0 < 5, f"{task.payload['goal']} did not halt"
                await asyncio.sleep(0.05)
    finally:
        shutdown.set()
        await run


async def test_a_worker_booting_into_a_stopped_workspace_stays_idle(
    db: Sessions, tmp_path: Path
) -> None:
    """A restarted or newly-scaled worker must observe the stop on its FIRST
    claim attempt — it has no NOTIFY history to replay, so the durable row is
    what has to carry the state."""
    async with session_scope(db) as session:
        await set_emergency_stop(
            session, workspace_id=DEFAULT_WORKSPACE_ID, stopped=True, reason="halt"
        )
    task = await _enqueue(db, "queued before the worker existed")

    worker = Worker(db, _config(tmp_path, worker_id="fresh"), _SlowLLM())
    shutdown = asyncio.Event()
    run = asyncio.ensure_future(worker.run(shutdown))
    try:
        await asyncio.sleep(0.5)
        assert await _status(db, task.id) is TaskStatus.PENDING
    finally:
        shutdown.set()
        await run


async def test_clearing_the_stop_lets_work_flow_again(db: Sessions, tmp_path: Path) -> None:
    """The switch must be reversible without restarting the fleet — the durable
    poll is what picks the change up even with no NOTIFY listener attached."""
    async with session_scope(db) as session:
        await set_emergency_stop(session, workspace_id=DEFAULT_WORKSPACE_ID, stopped=True)
    task = await _enqueue(db, "eventually runs")

    class _QuickLLM:
        async def complete(self, **kwargs: Any) -> Any:
            return final_response("done")

    worker = Worker(db, _config(tmp_path), _QuickLLM())
    shutdown = asyncio.Event()
    run = asyncio.ensure_future(worker.run(shutdown))
    loop = asyncio.get_running_loop()
    try:
        await asyncio.sleep(0.3)
        assert await _status(db, task.id) is TaskStatus.PENDING

        async with session_scope(db) as session:
            await set_emergency_stop(session, workspace_id=DEFAULT_WORKSPACE_ID, stopped=False)

        deadline = loop.time() + 10
        while await _status(db, task.id) is not TaskStatus.SUCCEEDED:
            assert loop.time() < deadline, "work did not resume after the stop was cleared"
            await asyncio.sleep(0.05)
    finally:
        shutdown.set()
        await run


async def test_halted_tasks_keep_their_log_and_can_be_retried(db: Sessions, tmp_path: Path) -> None:
    """A halt is not a data loss: the event log survives, so a task retried after
    the stop is cleared resumes rather than starting over."""
    task = await _enqueue(db, "halted then retried")

    worker = Worker(db, _config(tmp_path), _SlowLLM())
    shutdown = asyncio.Event()
    run = asyncio.ensure_future(worker.run(shutdown))
    loop = asyncio.get_running_loop()
    try:
        deadline = loop.time() + 10
        while task.id not in worker._running:
            assert loop.time() < deadline
            await asyncio.sleep(0.02)
        async with session_scope(db) as session:
            await set_emergency_stop(session, workspace_id=DEFAULT_WORKSPACE_ID, stopped=True)
        while await _status(db, task.id) is not TaskStatus.CANCELLED:
            assert loop.time() - deadline < 10
            await asyncio.sleep(0.05)
    finally:
        shutdown.set()
        await run

    async with session_scope(db) as session:
        events = await session.scalar(
            sa.select(sa.func.count()).select_from(TaskEvent).where(TaskEvent.task_id == task.id)
        )
        assert events and events > 0  # the trace survived the halt
        retried = await queue.retry(session, task_id=task.id)
    assert retried is not None and retried.status is TaskStatus.PENDING


async def test_notify_flips_the_gate_without_waiting_for_the_refresh(
    db: Sessions, tmp_path: Path
) -> None:
    """NOTIFY is the latency shortcut on top of the durable poll: with the
    refresh interval set deliberately long, only the notification can explain a
    prompt stop. This is what keeps "stop everything" instant at fleet scale
    instead of one poll interval per worker."""
    from gantry.core.notify import WORKSPACE_CONTROL_CHANNEL, QueueListener

    from .conftest import _test_database_url

    task = await _enqueue(db, "stopped by notify")
    # 60s poll: if the gate flips promptly, it was the notification.
    config = _config(tmp_path, worker_id="notify-w")
    config = replace(config, control_refresh_seconds=60.0)

    async with QueueListener(
        _test_database_url(), channel=WORKSPACE_CONTROL_CHANNEL
    ) as control_listener:
        worker = Worker(db, config, _SlowLLM(), control_listener=control_listener)
        shutdown = asyncio.Event()
        run = asyncio.ensure_future(worker.run(shutdown))
        loop = asyncio.get_running_loop()
        try:
            deadline = loop.time() + 10
            while task.id not in worker._running:
                assert loop.time() < deadline, "agent never started"
                await asyncio.sleep(0.02)

            t0 = loop.time()
            async with session_scope(db) as session:
                await set_emergency_stop(
                    session, workspace_id=DEFAULT_WORKSPACE_ID, stopped=True, reason="notify"
                )
            while await _status(db, task.id) is not TaskStatus.CANCELLED:
                assert loop.time() - t0 < 5, "the NOTIFY did not flip the gate"
                await asyncio.sleep(0.05)
        finally:
            shutdown.set()
            await run


# --- API contract ---------------------------------------------------------


async def test_emergency_stop_endpoints(client: httpx.AsyncClient) -> None:
    assert (await client.get("/api/control/emergency-stop")).json()["stopped"] is False

    tripped = await client.post(
        "/api/control/emergency-stop", json={"stopped": True, "reason": "budget breach"}
    )
    assert tripped.status_code == 200
    body = tripped.json()
    assert body["stopped"] is True
    assert body["reason"] == "budget breach"
    assert body["actor"]  # attributed, so an incident review knows who stopped it

    assert (await client.get("/api/control/emergency-stop")).json()["stopped"] is True

    cleared = await client.post("/api/control/emergency-stop", json={"stopped": False})
    assert cleared.json()["stopped"] is False
    assert (await client.get("/api/control/emergency-stop")).json()["stopped"] is False


async def test_stopping_through_the_api_halts_a_live_worker(
    client: httpx.AsyncClient, db: Sessions, tmp_path: Path
) -> None:
    """The full operator path: one HTTP call stops a running swarm."""
    task = await _enqueue(db, "stopped via the API")

    worker = Worker(db, _config(tmp_path), _SlowLLM())
    shutdown = asyncio.Event()
    run = asyncio.ensure_future(worker.run(shutdown))
    loop = asyncio.get_running_loop()
    try:
        deadline = loop.time() + 10
        while task.id not in worker._running:
            assert loop.time() < deadline, "agent never started"
            await asyncio.sleep(0.02)

        resp = await client.post(
            "/api/control/emergency-stop", json={"stopped": True, "reason": "stop the swarm"}
        )
        assert resp.status_code == 200

        t0 = loop.time()
        while await _status(db, task.id) is not TaskStatus.CANCELLED:
            assert loop.time() - t0 < 5, "worker kept running after the API stop"
            await asyncio.sleep(0.05)
    finally:
        shutdown.set()
        await run
