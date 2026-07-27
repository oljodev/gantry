"""Early leader wakeup on child failure.

A parked leader used to wait for EVERY child before it could react. Now a child
that FAILS or is CANCELLED wakes it immediately (queue-level), and
wait_for_children returns right away with the failure instead of blocking on the
survivors — but surfaces each failure only once, so the leader can respawn a fix
and go back to waiting for the rest.
"""

from __future__ import annotations

import json

import pytest
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.models import TaskStatus
from gantry.runtime.tools import TaskParked
from gantry.worker.tools.orchestration import WaitForChildrenTool

from .test_orchestration import ctx_for, get_status, make_planner, spawn

Sessions = async_sessionmaker[AsyncSession]


async def _fail_child(db: Sessions, child_id: object, *, worker_id: str = "cw") -> None:
    async with session_scope(db) as session:
        claimed = await queue.claim(session, worker_id=worker_id)
    assert claimed is not None and claimed.id == child_id, "claim took the wrong task"
    async with session_scope(db) as session:
        status = await queue.fail(
            session,
            task_id=claimed.id,
            worker_id=worker_id,
            attempt=claimed.attempt,
            error="boom",
            retryable=False,  # terminal FAILED, no retry
        )
    assert status is TaskStatus.FAILED


async def test_parked_leader_wakes_early_when_a_child_fails(db: Sessions) -> None:
    leader = await make_planner(db)  # claimed
    child_a = await spawn(db, leader, "call_A")
    child_b = await spawn(db, leader, "call_B")

    async with session_scope(db) as session:
        parked = await queue.park_for_children(
            session, task_id=leader.id, worker_id="planner-w", attempt=leader.attempt
        )
    assert parked is TaskStatus.WAITING_CHILDREN

    # Child A fails; child B has not even started.
    await _fail_child(db, child_a.id)

    # The leader is re-queued immediately — it does NOT wait for child B.
    assert await get_status(db, leader.id) is TaskStatus.PENDING
    assert await get_status(db, child_b.id) is TaskStatus.PENDING


async def test_leader_stays_parked_when_a_child_merely_succeeds(db: Sessions) -> None:
    # A partial SUCCESS is not actionable early — only a failure wakes the leader.
    leader = await make_planner(db)
    child_a = await spawn(db, leader, "call_A")
    await spawn(db, leader, "call_B")
    async with session_scope(db) as session:
        assert (
            await queue.park_for_children(
                session, task_id=leader.id, worker_id="planner-w", attempt=leader.attempt
            )
            is TaskStatus.WAITING_CHILDREN
        )

    async with session_scope(db) as session:
        claimed = await queue.claim(session, worker_id="cw")
    assert claimed is not None and claimed.id == child_a.id
    async with session_scope(db) as session:
        assert await queue.complete(
            session, task_id=child_a.id, worker_id="cw", attempt=claimed.attempt
        )

    # Still parked — one child succeeded but the other is unfinished.
    assert await get_status(db, leader.id) is TaskStatus.WAITING_CHILDREN


async def test_wait_returns_early_then_blocks_for_survivors(db: Sessions) -> None:
    leader = await make_planner(db)
    child_a = await spawn(db, leader, "call_A")
    child_b = await spawn(db, leader, "call_B")
    await _fail_child(db, child_a.id)

    # First wait: returns EARLY with the failure instead of parking.
    result = await WaitForChildrenTool().execute({}, ctx_for(leader, db, "wait_1"))
    assert not result.is_error
    report = json.loads(result.content)
    assert report["early_exit"] == "child_failure"
    assert report["newly_failed"][0]["task_id"] == str(child_a.id)
    assert "boom" in report["newly_failed"][0]["error"]
    assert [r["task_id"] for r in report["still_running"]] == [str(child_b.id)]

    # Second wait: no NEW failure and B still runs -> park (wait for the survivor).
    with pytest.raises(TaskParked):
        await WaitForChildrenTool().execute({}, ctx_for(leader, db, "wait_2"))

    # Finish B -> the next wait returns the full settled report.
    async with session_scope(db) as session:
        claimed_b = await queue.claim(session, worker_id="cw2")
    assert claimed_b is not None and claimed_b.id == child_b.id
    async with session_scope(db) as session:
        assert await queue.complete(
            session, task_id=child_b.id, worker_id="cw2", attempt=claimed_b.attempt
        )
    final = await WaitForChildrenTool().execute({}, ctx_for(leader, db, "wait_3"))
    full = json.loads(final.content)
    assert full["summary"] == {"failed": 1, "succeeded": 1}


async def test_a_second_distinct_failure_is_surfaced_again(db: Sessions) -> None:
    # After acknowledging A, a NEW failure (B) must wake/return early too.
    leader = await make_planner(db)
    child_a = await spawn(db, leader, "call_A")
    child_b = await spawn(db, leader, "call_B")
    await spawn(db, leader, "call_C")
    await _fail_child(db, child_a.id)

    first = json.loads((await WaitForChildrenTool().execute({}, ctx_for(leader, db, "w1"))).content)
    assert [r["task_id"] for r in first["newly_failed"]] == [str(child_a.id)]

    await _fail_child(db, child_b.id, worker_id="cw2")
    second = json.loads(
        (await WaitForChildrenTool().execute({}, ctx_for(leader, db, "w2"))).content
    )
    # Only B is NEW; A was already surfaced.
    assert [r["task_id"] for r in second["newly_failed"]] == [str(child_b.id)]
    assert second["summary"]["failed"] == 2
