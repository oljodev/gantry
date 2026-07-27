"""cancel_tree: stopping a task cancels its whole subtree, so a leader's spawned
workers can't linger as zombies. Leased (running) tasks get cooperative cancel +
a NOTIFY so their worker hard-cancels the slot; pending/parked tasks flip straight
to CANCELLED. Scoped to the subtree, and idempotent."""

from __future__ import annotations

from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.events import read_events
from gantry.core.models import DEFAULT_WORKSPACE_ID, EventType, Task, TaskKind, TaskStatus

from .test_queue import enqueue_one, get_task

Sessions = async_sessionmaker[AsyncSession]


async def _child(db: Sessions, parent: Task) -> Task:
    async with session_scope(db) as session:
        row = await session.get(Task, parent.id)
        assert row is not None
        return await queue.enqueue(
            session,
            workspace_id=DEFAULT_WORKSPACE_ID,
            kind=TaskKind.EXECUTE,
            payload={},
            parent=row,
        )


async def test_cancel_tree_cancels_the_whole_subtree(db: Sessions) -> None:
    leader = await enqueue_one(db)
    c1 = await _child(db, leader)
    c2 = await _child(db, leader)
    grandchild = await _child(db, c1)  # exercises the recursive walk

    async with session_scope(db) as session:
        affected = await queue.cancel_tree(session, task_id=leader.id)

    assert set(affected) == {leader.id, c1.id, c2.id, grandchild.id}
    for tid in (leader.id, c1.id, c2.id, grandchild.id):
        assert (await get_task(db, tid)).status is TaskStatus.CANCELLED
    async with session_scope(db) as session:
        events = await read_events(session, grandchild.id)
    assert any(e.event_type is EventType.TASK_CANCELLED for e in events)


async def test_cancel_tree_requests_cooperative_stop_for_a_running_task(db: Sessions) -> None:
    leader = await enqueue_one(db)
    child = await _child(db, leader)
    # Claim takes the earliest pending task — the leader — leaving it leased.
    async with session_scope(db) as session:
        claimed = await queue.claim(session, worker_id="w")
    assert claimed is not None and claimed.id == leader.id

    async with session_scope(db) as session:
        affected = await queue.cancel_tree(session, task_id=leader.id)

    assert set(affected) == {leader.id, child.id}
    leader_row = await get_task(db, leader.id)
    # A leased task can't be yanked terminal from under its worker: it is asked to
    # stop cooperatively (the NOTIFY hard-cancels the slot), so it stays CLAIMED.
    assert leader_row.status is TaskStatus.CLAIMED
    assert leader_row.cancel_requested is True
    # The pending child, owned by nobody, flips straight to terminal.
    assert (await get_task(db, child.id)).status is TaskStatus.CANCELLED


async def test_cancel_tree_is_idempotent_for_a_terminal_subtree(db: Sessions) -> None:
    leader = await enqueue_one(db)
    child = await _child(db, leader)
    async with session_scope(db) as session:
        first = await queue.cancel_tree(session, task_id=leader.id)
    assert set(first) == {leader.id, child.id}
    async with session_scope(db) as session:
        second = await queue.cancel_tree(session, task_id=leader.id)
    assert second == []  # already terminal — nothing to cancel again


async def test_cancel_tree_only_touches_its_own_subtree(db: Sessions) -> None:
    run_a = await enqueue_one(db)
    a_child = await _child(db, run_a)
    run_b = await enqueue_one(db)
    b_child = await _child(db, run_b)

    async with session_scope(db) as session:
        affected = await queue.cancel_tree(session, task_id=run_a.id)

    assert set(affected) == {run_a.id, a_child.id}
    assert (await get_task(db, run_b.id)).status is TaskStatus.PENDING
    assert (await get_task(db, b_child.id)).status is TaskStatus.PENDING


async def test_cancel_tree_from_a_mid_tree_node_spares_the_parent(db: Sessions) -> None:
    leader = await enqueue_one(db)
    child = await _child(db, leader)
    grandchild = await _child(db, child)

    async with session_scope(db) as session:
        affected = await queue.cancel_tree(session, task_id=child.id)

    assert set(affected) == {child.id, grandchild.id}
    assert (await get_task(db, leader.id)).status is TaskStatus.PENDING
