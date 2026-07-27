"""Milestone 2: hierarchical sub-leaders and concise trace titles.

A leader can delegate a large sub-scope to a SUB-LEADER — a spawned plan task
that runs the swarm workflow one level down. The `role` field is the friendly
knob (role='sub_leader' -> kind=plan); a spawned plan task is marked
``sub_leader`` so the UI labels it, and the role predicates (`_delegates`,
`max_steps_for`) treat it like any delegating leader so it gets the leader step
budget and compaction. A `title` rides along for a tight trace-tree label.
"""

from __future__ import annotations

from typing import Any

from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.config import Settings
from gantry.core.models import Task, TaskKind
from gantry.prompts import PLANNER_SYSTEM_PROMPT
from gantry.runtime.loop import _delegates, max_steps_for
from gantry.worker.tools.orchestration import SpawnBatchTool, SpawnSubtaskTool, child_task_id

from .test_orchestration import _children_of, _planner_with, ctx_for, make_planner

Sessions = async_sessionmaker[AsyncSession]


async def _spawn(db: Sessions, parent: Task, call_id: str, **args: Any) -> Task:
    result = await SpawnSubtaskTool().execute(
        {"goal": f"child for {call_id}", **args}, ctx_for(parent, db, call_id)
    )
    assert not result.is_error, result.content
    async with db() as session:
        child = await session.get(Task, child_task_id(parent.id, call_id))
    assert child is not None
    return child


async def test_role_sub_leader_spawns_a_delegating_plan_task(db: Sessions) -> None:
    leader = await make_planner(db)
    sub = await _spawn(db, leader, "call_S", role="sub_leader")

    assert sub.kind is TaskKind.PLAN
    # Marked as a sub-leader, and seeded with the leader/orchestrator prompt.
    assert sub.payload["sub_leader"] is True
    assert sub.payload["system_prompt"] == PLANNER_SYSTEM_PROMPT
    # The role predicates treat it as a delegating leader — so it gets the leader
    # step budget and compaction, not a leaf worker's tight one.
    settings = Settings()
    assert _delegates(sub) is True
    assert max_steps_for(sub, settings) == settings.leader_max_steps


async def test_role_worker_is_the_default_leaf(db: Sessions) -> None:
    # Under an autonomous leader, a plain worker is a tight-budget leaf micro-task
    # (non_interactive flows down), NOT a delegating agent.
    leader = await _planner_with(db, autonomous_leader=True)
    worker = await _spawn(db, leader, "call_W", role="worker")
    assert worker.kind is TaskKind.EXECUTE
    assert "sub_leader" not in worker.payload
    assert _delegates(worker) is False
    settings = Settings()
    assert max_steps_for(worker, settings) == settings.execute_max_steps


async def test_explicit_kind_plan_is_still_marked_a_sub_leader(db: Sessions) -> None:
    # The low-level `kind` knob still works and yields the same marked sub-leader.
    leader = await make_planner(db)
    sub = await _spawn(db, leader, "call_K", kind="plan")
    assert sub.kind is TaskKind.PLAN
    assert sub.payload["sub_leader"] is True


async def test_title_is_stored_on_the_child_payload(db: Sessions) -> None:
    leader = await make_planner(db)
    child = await _spawn(db, leader, "call_T", title="Refactor board.py")
    assert child.payload["title"] == "Refactor board.py"
    # A blank title falls through — nothing stored, so the UI derives one.
    plain = await _spawn(db, leader, "call_T2", title="   ")
    assert "title" not in plain.payload


async def test_spawn_batch_honors_role_and_title(db: Sessions) -> None:
    leader = await make_planner(db)
    result = await SpawnBatchTool().execute(
        {
            "children": [
                {
                    "goal": "coordinate the api subsystem",
                    "role": "sub_leader",
                    "title": "API layer",
                },
                {"goal": "tweak one file", "title": "Fix util"},
            ]
        },
        ctx_for(leader, db, "call_B"),
    )
    assert not result.is_error, result.content
    kids = {c.payload["goal"]: c for c in await _children_of(db, leader.id)}
    sub = kids["coordinate the api subsystem"]
    assert sub.kind is TaskKind.PLAN and sub.payload["sub_leader"] is True
    assert sub.payload["title"] == "API layer"
    worker = kids["tweak one file"]
    assert worker.kind is TaskKind.EXECUTE and worker.payload["title"] == "Fix util"
