"""Phase 6 orchestration: spawn fan-out, event-driven dormancy, and wakeups.

The flagship test runs a planner and a fleet of workers against real Postgres:
the planner spawns three subtasks, parks at zero compute, is woken by the last
child's completion, and integrates the children's results — with every park,
wake, and hand-off durable in the event log.
"""

from __future__ import annotations

import asyncio
import json
import uuid
from typing import Any

import pytest
import sqlalchemy as sa
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.models import DEFAULT_WORKSPACE_ID, Task, TaskEvent, TaskKind, TaskStatus
from gantry.runtime.tools import TaskParked, ToolContext
from gantry.worker.service import Worker, WorkerConfig
from gantry.worker.tools.orchestration import (
    MAX_SPAWN_DEPTH,
    AgentStatusTool,
    AgentTerminateTool,
    SpawnBatchTool,
    SpawnSubtaskTool,
    WaitForChildrenTool,
    _is_unclonable_local_repo,
    batch_child_id,
    child_task_id,
)

from .fakes import OrchestratorLLM

Sessions = async_sessionmaker[AsyncSession]


async def make_planner(db: Sessions, *, claim: bool = True) -> Task:
    async with session_scope(db) as session:
        task = await queue.enqueue(
            session,
            workspace_id=DEFAULT_WORKSPACE_ID,
            kind=TaskKind.PLAN,
            payload={"goal": "PLAN: orchestrate"},
            max_attempts=20,
        )
    if claim:
        async with session_scope(db) as session:
            claimed = await queue.claim(session, worker_id="planner-w", lease_seconds=30)
        assert claimed is not None and claimed.id == task.id
        return claimed
    return task


def ctx_for(task: Task, db: Sessions, call_id: str) -> ToolContext:
    return ToolContext(task_id=task.id, sessions=db, tool_call_id=call_id)


async def spawn(db: Sessions, parent: Task, call_id: str, **args: Any) -> Task:
    result = await SpawnSubtaskTool().execute(
        {"goal": f"child for {call_id}", **args}, ctx_for(parent, db, call_id)
    )
    assert not result.is_error, result.content
    async with db() as session:
        child = await session.get(Task, child_task_id(parent.id, call_id))
    assert child is not None
    return child


async def get_status(db: Sessions, task_id: uuid.UUID) -> TaskStatus:
    async with db() as session:
        status = await session.scalar(sa.select(Task.status).where(Task.id == task_id))
    assert status is not None
    return TaskStatus(status)


async def event_types_of(db: Sessions, task_id: uuid.UUID) -> list[str]:
    async with db() as session:
        rows = await session.scalars(
            sa.select(TaskEvent.event_type)
            .where(TaskEvent.task_id == task_id)
            .order_by(TaskEvent.seq)
        )
    return [str(r) for r in rows]


async def _batch(
    db: Sessions, parent: Task, call_id: str, children: list[dict[str, Any]]
) -> dict[str, Any]:
    result = await SpawnBatchTool().execute({"children": children}, ctx_for(parent, db, call_id))
    assert not result.is_error, result.content
    return json.loads(result.content)  # type: ignore[no-any-return]


async def _children_of(db: Sessions, parent_id: uuid.UUID) -> list[Task]:
    async with db() as session:
        return list(
            (await session.scalars(sa.select(Task).where(Task.parent_task_id == parent_id))).all()
        )


async def test_spawn_batch_creates_the_whole_burst_in_one_call(db: Sessions) -> None:
    planner = await make_planner(db)
    report = await _batch(db, planner, "call_B", [{"goal": "g0"}, {"goal": "g1"}, {"goal": "g2"}])
    assert report["count"] == 3
    kids = await _children_of(db, planner.id)
    assert len(kids) == 3
    # Each id is the nested batch id — distinct, and non-aliasing with a spawn_subtask id.
    expected = {batch_child_id(planner.id, "call_B", i) for i in range(3)}
    assert {k.id for k in kids} == expected
    assert child_task_id(planner.id, "call_B") not in expected
    # All created in one transaction -> a tight created_at window.
    assert len({k.payload["goal"] for k in kids}) == 3


async def test_spawn_batch_is_exactly_once_and_converges_on_replay(db: Sessions) -> None:
    planner = await make_planner(db)
    children = [{"goal": "g0"}, {"goal": "g1"}]
    first = await _batch(db, planner, "call_B", children)
    # A crash-recovery re-run (same call id) re-derives the same ids and inserts
    # nothing new — converges, no duplicates.
    again = await _batch(db, planner, "call_B", children)
    assert first["spawned"] == again["spawned"]
    assert len(await _children_of(db, planner.id)) == 2


async def test_spawn_batch_cap_is_a_pure_function_of_pre_batch_state(db: Sessions) -> None:
    planner = await make_planner(db)
    # A cap of 2, batch of 4: only the first 2 (base_count 0 + i < 2) are created.
    tool = SpawnBatchTool(max_subtasks=2)
    result = await tool.execute(
        {"children": [{"goal": f"g{i}"} for i in range(4)]}, ctx_for(planner, db, "call_B")
    )
    report = json.loads(result.content)
    assert report["count"] == 2
    assert report["skipped"]["indices"] == [2, 3]
    # Replaying the identical batch accepts the identical prefix (the two already
    # exist; the cap still rejects 2 and 3) — no drift.
    again = json.loads(
        (
            await tool.execute(
                {"children": [{"goal": f"g{i}"} for i in range(4)]}, ctx_for(planner, db, "call_B")
            )
        ).content
    )
    assert again["count"] == 2 and again["skipped"]["indices"] == [2, 3]


async def test_spawn_batch_child_carries_no_parent_history_and_honors_model(db: Sessions) -> None:
    planner = await make_planner(db)
    await _batch(db, planner, "call_B", [{"goal": "g0", "model": "deepseek/deepseek-chat"}])
    child = (await _children_of(db, planner.id))[0]
    assert child.payload["goal"] == "g0"
    assert child.payload["model"] == "deepseek/deepseek-chat"
    assert "team" not in child.payload and "tracked" not in child.payload


async def test_spawn_batch_rejects_a_bad_spec_without_partial_spawn(db: Sessions) -> None:
    planner = await make_planner(db)
    result = await SpawnBatchTool().execute(
        {"children": [{"goal": "ok"}, {"goal": ""}]}, ctx_for(planner, db, "call_B")
    )
    assert result.is_error and "children[1]" in result.content
    assert await _children_of(db, planner.id) == []  # nothing spawned


async def test_spawn_subtask_is_exactly_once(db: Sessions) -> None:
    planner = await make_planner(db)
    child = await spawn(db, planner, "call_A", priority=7)

    assert child.parent_task_id == planner.id
    assert child.root_task_id == planner.id  # one tree, one indexed query
    assert child.workspace_id == planner.workspace_id
    assert child.kind is TaskKind.EXECUTE
    assert child.priority == 7
    assert child.payload["goal"] == "child for call_A"

    # Crash-recovery re-run of the SAME call converges on the SAME child.
    rerun = await SpawnSubtaskTool().execute(
        {"goal": "child for call_A"}, ctx_for(planner, db, "call_A")
    )
    assert not rerun.is_error and "already spawned" in rerun.content
    async with db() as session:
        count = await session.scalar(
            sa.select(sa.func.count()).select_from(Task).where(Task.parent_task_id == planner.id)
        )
    assert count == 1


async def test_spawn_respects_subtask_cap(db: Sessions) -> None:
    planner = await make_planner(db)
    await spawn(db, planner, "call_A")
    capped = await SpawnSubtaskTool(max_subtasks=1).execute(
        {"goal": "one too many"}, ctx_for(planner, db, "call_B")
    )
    assert capped.is_error and "cap" in capped.content


async def test_wait_without_children_does_not_park(db: Sessions) -> None:
    planner = await make_planner(db)
    result = await WaitForChildrenTool().execute({}, ctx_for(planner, db, "wait_1"))
    assert not result.is_error and "no subtasks" in result.content


async def test_wait_parks_while_children_run_and_reports_when_settled(db: Sessions) -> None:
    planner = await make_planner(db)
    child = await spawn(db, planner, "call_A")

    with pytest.raises(TaskParked):
        await WaitForChildrenTool().execute({}, ctx_for(planner, db, "wait_1"))

    async with session_scope(db) as session:
        claimed = await queue.claim(session, worker_id="child-w", lease_seconds=30)
    assert claimed is not None and claimed.id == child.id
    async with session_scope(db) as session:
        assert await queue.complete(
            session,
            task_id=child.id,
            worker_id="child-w",
            attempt=claimed.attempt,
            result={"final_text": "child says hi", "branch": "gantry/task-x"},
        )

    result = await WaitForChildrenTool().execute({}, ctx_for(planner, db, "wait_1"))
    assert not result.is_error
    report = json.loads(result.content)
    assert report["summary"] == {"succeeded": 1}
    assert report["children"][0]["final_text"] == "child says hi"
    assert report["children"][0]["branch"] == "gantry/task-x"


async def test_last_finishing_child_wakes_the_parked_parent(db: Sessions) -> None:
    planner = await make_planner(db)
    child_a = await spawn(db, planner, "call_A")
    child_b = await spawn(db, planner, "call_B")

    async with session_scope(db) as session:
        status = await queue.park_for_children(
            session, task_id=planner.id, worker_id="planner-w", attempt=planner.attempt
        )
    assert status is TaskStatus.WAITING_CHILDREN

    for i, child in enumerate([child_a, child_b]):
        async with session_scope(db) as session:
            claimed = await queue.claim(session, worker_id=f"w{i}", lease_seconds=30)
        assert claimed is not None
        async with session_scope(db) as session:
            assert await queue.complete(
                session, task_id=claimed.id, worker_id=f"w{i}", attempt=claimed.attempt
            )
        expected = TaskStatus.WAITING_CHILDREN if child is child_a else TaskStatus.PENDING
        assert await get_status(db, planner.id) is expected, f"after finishing {i + 1} children"

    types = await event_types_of(db, planner.id)
    assert "task_parked" in types and "task_resumed" in types


async def test_simultaneous_final_children_still_wake_parent(db: Sessions) -> None:
    """Write-skew regression (caught live in the Phase 6 demo): two siblings
    completing in OVERLAPPING transactions must not each see the other as
    unfinished and both skip the wake. The parent-row lock serializes them."""
    planner = await make_planner(db)
    child_a = await spawn(db, planner, "call_A")
    child_b = await spawn(db, planner, "call_B")
    async with session_scope(db) as session:
        assert (
            await queue.park_for_children(
                session, task_id=planner.id, worker_id="planner-w", attempt=planner.attempt
            )
            is TaskStatus.WAITING_CHILDREN
        )

    claims: dict[uuid.UUID, tuple[str, int]] = {}
    for i in range(2):
        async with session_scope(db) as session:
            claimed = await queue.claim(session, worker_id=f"cw{i}", lease_seconds=30)
        assert claimed is not None
        claims[claimed.id] = (f"cw{i}", claimed.attempt)

    async def complete_holding_txn_open(child_id: uuid.UUID) -> None:
        worker_id, attempt = claims[child_id]
        async with db() as session:
            assert await queue.complete(
                session, task_id=child_id, worker_id=worker_id, attempt=attempt
            )
            await asyncio.sleep(0.3)  # keep the transaction open so both overlap
            await session.commit()

    await asyncio.wait_for(
        asyncio.gather(
            complete_holding_txn_open(child_a.id),
            complete_holding_txn_open(child_b.id),
        ),
        timeout=10,
    )
    assert await get_status(db, planner.id) is TaskStatus.PENDING, "wakeup was lost to write skew"


async def test_lost_wakeup_guard_unparks_immediately(db: Sessions) -> None:
    """Children all finished between the wait-check and the park commit."""
    planner = await make_planner(db)
    child = await spawn(db, planner, "call_A")

    # The child settles while the planner is still nominally running (no wake).
    async with session_scope(db) as session:
        claimed = await queue.claim(session, worker_id="fast-child", lease_seconds=30)
    assert claimed is not None and claimed.id == child.id
    async with session_scope(db) as session:
        assert await queue.complete(
            session, task_id=child.id, worker_id="fast-child", attempt=claimed.attempt
        )
    # Parking now must detect the settled children and immediately re-queue.
    async with session_scope(db) as session:
        status = await queue.park_for_children(
            session, task_id=planner.id, worker_id="planner-w", attempt=planner.attempt
        )
    assert status is TaskStatus.PENDING


async def test_reaped_terminal_child_wakes_parent(db: Sessions) -> None:
    planner = await make_planner(db)
    await spawn(db, planner, "call_A", max_attempts=1)

    async with session_scope(db) as session:
        child = await queue.claim(session, worker_id="doomed", lease_seconds=0.05)
    assert child is not None
    async with session_scope(db) as session:
        status = await queue.park_for_children(
            session, task_id=planner.id, worker_id="planner-w", attempt=planner.attempt
        )
    assert status is TaskStatus.WAITING_CHILDREN

    await asyncio.sleep(0.2)  # let the child's lease lapse
    async with session_scope(db) as session:
        reaped = await queue.reap_expired(session)
    assert [r.status for r in reaped] == [TaskStatus.FAILED]  # max_attempts=1 → terminal
    assert await get_status(db, planner.id) is TaskStatus.PENDING


async def test_cancelled_child_wakes_parent(db: Sessions) -> None:
    planner = await make_planner(db)
    child = await spawn(db, planner, "call_A")
    async with session_scope(db) as session:
        status = await queue.park_for_children(
            session, task_id=planner.id, worker_id="planner-w", attempt=planner.attempt
        )
    assert status is TaskStatus.WAITING_CHILDREN
    async with session_scope(db) as session:
        assert await queue.cancel(session, task_id=child.id) is not None
    assert await get_status(db, planner.id) is TaskStatus.PENDING


async def test_planner_fleet_end_to_end(db: Sessions, tmp_path: Any) -> None:
    """Goal in → plan → parallel workers → park → wake → integrated result."""
    goals = ["alpha", "beta", "gamma"]
    llm = OrchestratorLLM(subtask_goals=goals, child_delay_seconds=0.1)
    planner_task = await make_planner(db, claim=False)

    workers = [
        Worker(
            db,
            WorkerConfig(
                worker_id=f"fleet-{i}",
                workspace_root=tmp_path / f"ws{i}",
                lease_seconds=30,
                poll_interval_seconds=0.05,
            ),
            llm,
        )
        for i in range(3)
    ]
    shutdown = asyncio.Event()
    runs = [asyncio.create_task(w.run(shutdown)) for w in workers]
    try:
        deadline = asyncio.get_running_loop().time() + 30
        while await get_status(db, planner_task.id) is not TaskStatus.SUCCEEDED:
            assert asyncio.get_running_loop().time() < deadline, "planner never finished"
            await asyncio.sleep(0.1)
    finally:
        shutdown.set()
        await asyncio.gather(*runs)

    async with db() as session:
        planner = await session.get(Task, planner_task.id)
        assert planner is not None
        children = (
            await session.scalars(
                sa.select(Task).where(Task.parent_task_id == planner.id).order_by(Task.created_at)
            )
        ).all()

    # Exactly one child per spawn call — no duplicates from any re-run.
    assert len(children) == 3
    assert all(c.status is TaskStatus.SUCCEEDED for c in children)
    assert all(c.root_task_id == planner.id for c in children)
    assert {c.payload["goal"] for c in children} == set(goals)

    # The planner's final answer integrates every child's result.
    assert planner.result is not None
    final_text = planner.result["final_text"]
    assert final_text.startswith("INTEGRATED:")
    for goal in goals:
        assert f"answer[{goal}]" in final_text

    # The dormancy cycle is durable in the log.
    types = await event_types_of(db, planner.id)
    parked_at = types.index("task_parked")
    assert "task_resumed" in types[parked_at:]  # woken after parking, durably logged


async def _planner_with(db: Sessions, **payload_extra: Any) -> Task:
    async with session_scope(db) as session:
        await queue.enqueue(
            session,
            workspace_id=DEFAULT_WORKSPACE_ID,
            kind=TaskKind.PLAN,
            payload={"goal": "PLAN: orchestrate", **payload_extra},
            max_attempts=20,
        )
    async with session_scope(db) as session:
        claimed = await queue.claim(session, worker_id="planner-w", lease_seconds=30)
    assert claimed is not None
    return claimed


async def test_child_inherits_parent_provider_and_model(db: Sessions) -> None:
    """A delegated child must reuse the planner's credentials, or it falls back
    to the keyless server default and fails with an auth error."""
    planner = await _planner_with(
        db, provider_id="11111111-1111-4111-8111-111111111111", model="openrouter/deepseek/x"
    )
    child = await spawn(db, planner, "call_A")
    assert child.payload["provider_id"] == "11111111-1111-4111-8111-111111111111"
    assert child.payload["model"] == "openrouter/deepseek/x"


async def test_child_inherits_parent_repo_url(db: Sessions) -> None:
    planner = await _planner_with(db, repo_url="https://github.com/oljodev/real.git")
    child = await spawn(db, planner, "call_A")
    assert child.payload["repo_url"] == "https://github.com/oljodev/real.git"


async def test_child_pinning_its_own_model_keeps_the_gateway_provider(db: Sessions) -> None:
    """A child that pins a cheaper model still inherits the parent's provider_id
    (the account/gateway key), so cost-tiered spawns run on the same key instead
    of falling off to the keyless server default. This is what makes per-child
    model override actually usable for a multi-model gateway like OpenRouter."""
    planner = await _planner_with(
        db, provider_id="11111111-1111-4111-8111-111111111111", model="deepseek/deepseek-r1"
    )
    child = await spawn(db, planner, "call_A", model="deepseek/deepseek-chat")
    # Its own (cheaper) model wins, but it runs on the parent's key.
    assert child.payload["model"] == "deepseek/deepseek-chat"
    assert child.payload["provider_id"] == "11111111-1111-4111-8111-111111111111"


async def test_child_can_pin_its_own_provider_and_model(db: Sessions) -> None:
    # A child that explicitly names a different provider keeps both — no inherit.
    planner = await _planner_with(
        db, provider_id="11111111-1111-4111-8111-111111111111", model="deepseek/deepseek-r1"
    )
    child = await spawn(
        db,
        planner,
        "call_A",
        model="anthropic/claude-opus-4-8",
        payload={"provider_id": "22222222-2222-4222-8222-222222222222"},
    )
    assert child.payload["model"] == "anthropic/claude-opus-4-8"
    assert child.payload["provider_id"] == "22222222-2222-4222-8222-222222222222"


async def test_blank_repo_url_falls_through_to_inheritance(db: Sessions) -> None:
    planner = await _planner_with(db, repo_url="https://github.com/oljodev/real.git")
    child = await spawn(db, planner, "call_A", repo_url="   ")
    assert child.payload["repo_url"] == "https://github.com/oljodev/real.git"


async def test_local_repo_url_falls_through_to_inheritance(db: Sessions) -> None:
    """A leader that fabricates a local/self-referential repo_url (e.g. the
    hallucinated ``file:///app/.git``) for a child must not fail the child at clone
    time: the bogus local path is dropped and the child inherits the parent's real
    repo, exactly as a blank repo_url does."""
    planner = await _planner_with(db, repo_url="https://github.com/oljodev/real.git")
    child = await spawn(db, planner, "call_A", repo_url="file:///app/.git")
    assert child.payload["repo_url"] == "https://github.com/oljodev/real.git"


def test_is_unclonable_local_repo_flags_local_but_not_real_remotes() -> None:
    for bad in (
        "file:///app/.git",
        "file:///home/olav/dev/gantry-testing/.git",
        "/app/.git",
        "./repo",
        "~/code/chess",
        "http://localhost:8400/repo.git",
        "ssh://127.0.0.1/srv/git/x",
        "localhost:some/path",
    ):
        assert _is_unclonable_local_repo(bad), bad
    for ok in (
        "https://github.com/oljodev/gantry-testing.git",
        "git@github.com:oljodev/real.git",
        "ssh://git@github.com/oljodev/real.git",
        "https://github.com/foo/localhost-tools.git",  # 'localhost' only in the path
    ):
        assert not _is_unclonable_local_repo(ok), ok


async def test_placeholder_repo_url_is_rejected(db: Sessions) -> None:
    planner = await make_planner(db)
    result = await SpawnSubtaskTool().execute(
        {"goal": "build it", "repo_url": "https://github.com/your-org/chess-game.git"},
        ctx_for(planner, db, "call_ph"),
    )
    assert result.is_error
    assert "placeholder" in result.content
    async with db() as session:
        child = await session.get(Task, child_task_id(planner.id, "call_ph"))
    assert child is None  # never created


async def test_spawn_sets_and_increments_child_depth(db: Sessions) -> None:
    planner = await make_planner(db)  # no depth in payload -> treated as 0
    child = await spawn(db, planner, "call_A")
    assert child.payload["depth"] == 1


async def test_spawn_depth_cap_rejects_too_deep(db: Sessions) -> None:
    planner = await _planner_with(db, depth=MAX_SPAWN_DEPTH)
    result = await SpawnSubtaskTool().execute(
        {"goal": "one level too deep"}, ctx_for(planner, db, "deep")
    )
    assert result.is_error and "depth cap" in result.content
    async with db() as session:
        child = await session.get(Task, child_task_id(planner.id, "deep"))
    assert child is None  # never created


async def test_agent_status_reports_a_child(db: Sessions) -> None:
    planner = await make_planner(db)
    child = await spawn(db, planner, "call_A")
    result = await AgentStatusTool().execute(
        {"task_id": str(child.id)}, ctx_for(planner, db, "status_1")
    )
    assert not result.is_error
    entry = json.loads(result.content)
    assert entry["task_id"] == str(child.id)
    assert entry["status"] == TaskStatus.PENDING.value


async def test_agent_status_rejects_a_non_child(db: Sessions) -> None:
    planner = await make_planner(db)
    other = await make_planner(db)  # a task that is NOT this planner's child
    result = await AgentStatusTool().execute(
        {"task_id": str(other.id)}, ctx_for(planner, db, "status_2")
    )
    assert result.is_error and "not one of your children" in result.content


async def test_agent_status_guides_a_made_up_id_back_to_the_real_one(db: Sessions) -> None:
    # The leader sometimes invents a readable name (e.g. "survey-board") instead
    # of the UUID spawn_subtask returned; the error must steer it back, not just
    # say "invalid".
    planner = await make_planner(db)
    result = await AgentStatusTool().execute(
        {"task_id": "survey-board"}, ctx_for(planner, db, "status_bad")
    )
    assert result.is_error
    assert "spawn_subtask" in result.content and "wait_for_children" in result.content


async def test_agent_terminate_stops_a_child(db: Sessions) -> None:
    planner = await make_planner(db)
    child = await spawn(db, planner, "call_A")
    result = await AgentTerminateTool().execute(
        {"task_id": str(child.id)}, ctx_for(planner, db, "term_1")
    )
    assert not result.is_error and "stop" in result.content
    assert await get_status(db, child.id) is TaskStatus.CANCELLED


async def test_agent_terminate_rejects_a_non_child(db: Sessions) -> None:
    planner = await make_planner(db)
    other = await make_planner(db)
    result = await AgentTerminateTool().execute(
        {"task_id": str(other.id)}, ctx_for(planner, db, "term_2")
    )
    assert result.is_error and "not one of your children" in result.content
    assert await get_status(db, other.id) is not TaskStatus.CANCELLED
