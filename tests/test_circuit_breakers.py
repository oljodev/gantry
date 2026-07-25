"""Death-loop circuit breakers: always-finite per-agent step caps and the spawn
refusals that stop a leader funding wave after wave of failing fixers.

These are the guards that make the ``gui.py`` benchmark's self-healing death loop
impossible by invariant: a delegating agent is never step-unbounded (it halts
gracefully), a spawned micro-task can't grind for dozens of steps, and a leader
whose children keep failing is refused more spawns instead of thrashing forever.
"""

from __future__ import annotations

import uuid
from typing import Any, ClassVar

import pytest
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.config import Settings
from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.models import DEFAULT_WORKSPACE_ID, Task, TaskKind, TaskStatus
from gantry.core.queue import failed_child_count, run_task_count
from gantry.runtime.llm import LLMResponse, LLMUsage, ToolCallRequest
from gantry.runtime.loop import (
    AgentLoopError,
    _delegates,
    max_steps_for,
    run_agent_task,
)
from gantry.runtime.tools import Tool, ToolContext, ToolRegistry, ToolResult
from gantry.worker.tools.orchestration import SpawnBatchTool, SpawnSubtaskTool, child_task_id

Sessions = async_sessionmaker[AsyncSession]

# --- Layer A: role-aware, always-finite step caps (pure functions) ------------

_CAPS = Settings(
    _env_file=None,
    execute_max_steps=25,
    leader_max_steps=150,
    default_max_steps=300,
)


def _task(kind: TaskKind, **payload: Any) -> Task:
    return Task(kind=kind, payload=payload)


def test_a_delegating_agent_is_never_step_unbounded() -> None:
    # The regression: a plan/can_spawn/leader used to get max_steps=None and could
    # loop forever. Every delegating role now gets the same FINITE leader budget.
    for t in (
        _task(TaskKind.PLAN),
        _task(TaskKind.EXECUTE, can_spawn=True),
        _task(TaskKind.EXECUTE, autonomous_leader=True),
    ):
        cap = max_steps_for(t, _CAPS)
        assert cap == 150 and isinstance(cap, int)


def test_an_autonomous_swarm_leaf_gets_the_tight_execute_cap() -> None:
    # A spawned, non-interactive micro-task is stuck long before 300 steps.
    assert max_steps_for(_task(TaskKind.EXECUTE, non_interactive=True), _CAPS) == 25


def test_a_standalone_interactive_leaf_keeps_the_default_cap() -> None:
    # A user's own single task is not a micro-task, so it keeps generous headroom.
    assert max_steps_for(_task(TaskKind.EXECUTE), _CAPS) == 300


def test_an_explicit_max_steps_overrides_every_role() -> None:
    assert max_steps_for(_task(TaskKind.PLAN, max_steps=7), _CAPS) == 7
    assert max_steps_for(_task(TaskKind.EXECUTE, non_interactive=True, max_steps=99), _CAPS) == 99


def test_the_delegation_predicate_matches_the_compaction_role() -> None:
    assert _delegates(_task(TaskKind.PLAN))
    assert _delegates(_task(TaskKind.EXECUTE, can_spawn=True))
    assert _delegates(_task(TaskKind.EXECUTE, autonomous_leader=True))
    assert not _delegates(_task(TaskKind.EXECUTE))
    assert not _delegates(_task(TaskKind.EXECUTE, non_interactive=True))


# --- Layer A: halt BEHAVIOR at the cap (leader graceful vs leaf fails) ---------


class _NoopTool(Tool):
    name = "noop"
    description = "does nothing; keeps the agent from ever finishing"
    parameters: ClassVar[dict[str, Any]] = {"type": "object", "properties": {}}

    async def execute(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult:
        return ToolResult("ok")


class _AlwaysCallsNoop:
    """A model that never finishes: every turn it calls the no-op tool, so the loop
    can only stop by hitting the step ceiling."""

    async def complete(
        self, *, model: str, messages: Any, tools: Any = (), on_delta: Any = None
    ) -> LLMResponse:
        return LLMResponse(
            content=None,
            tool_calls=(ToolCallRequest(id=uuid.uuid4().hex, name="noop", arguments={}),),
            usage=LLMUsage(prompt_tokens=10, completion_tokens=1),
            model=model,
            finish_reason="tool_calls",
        )


async def _enqueue(
    db: Sessions,
    *,
    kind: TaskKind = TaskKind.PLAN,
    parent: Task | None = None,
    status: TaskStatus | None = None,
    **payload: Any,
) -> Task:
    async with session_scope(db) as session:
        p = await session.get(Task, parent.id) if parent is not None else None
        task = await queue.enqueue(
            session,
            workspace_id=DEFAULT_WORKSPACE_ID,
            kind=kind,
            payload={"goal": "g", **payload},
            parent=p,
        )
        if status is not None:
            task.status = status
        await session.flush()
        return task


async def test_a_leader_halts_gracefully_at_the_step_cap(db: Sessions) -> None:
    task = await _enqueue(db, kind=TaskKind.PLAN, max_steps=3, autonomous_leader=True)
    outcome = await run_agent_task(
        db, task, _AlwaysCallsNoop(), ToolRegistry([_NoopTool()]), compaction=None
    )
    # Clean, terminal SUCCEEDED-style outcome with a report — not a hard FAIL.
    assert outcome.steps == 3
    assert "Halted: reached the step budget" in outcome.final_text


async def test_a_leaf_worker_fails_at_the_step_cap(db: Sessions) -> None:
    task = await _enqueue(db, kind=TaskKind.EXECUTE, max_steps=3, non_interactive=True)
    # A stuck leaf raises so its parent learns it failed (non-retryable, via caller).
    with pytest.raises(AgentLoopError):
        await run_agent_task(
            db, task, _AlwaysCallsNoop(), ToolRegistry([_NoopTool()]), compaction=None
        )


# --- Layers C + D: repair-wave and run-capacity spawn refusals -----------------


def _ctx(task: Task, db: Sessions, call_id: str) -> ToolContext:
    return ToolContext(task_id=task.id, sessions=db, tool_call_id=call_id)


async def _add_children(db: Sessions, parent: Task, n: int, status: TaskStatus) -> None:
    for i in range(n):
        await _enqueue(db, kind=TaskKind.EXECUTE, parent=parent, status=status, goal=f"c{i}")


async def test_failed_child_count_and_run_task_count_only_see_what_they_should(
    db: Sessions,
) -> None:
    parent = await _enqueue(db, kind=TaskKind.PLAN, depth=0)
    await _add_children(db, parent, 3, TaskStatus.FAILED)
    await _add_children(db, parent, 2, TaskStatus.SUCCEEDED)
    await _add_children(db, parent, 1, TaskStatus.CANCELLED)
    async with session_scope(db) as session:
        # Only FAILED count as repair evidence; cancelled/succeeded do not.
        assert await failed_child_count(session, parent.id) == 3
        # The whole tree: parent + 6 children.
        assert await run_task_count(session, parent.root_task_id) == 7


async def test_spawn_is_refused_after_enough_children_fail(db: Sessions) -> None:
    parent = await _enqueue(db, kind=TaskKind.PLAN, depth=0)
    await _add_children(db, parent, 5, TaskStatus.FAILED)
    tool = SpawnSubtaskTool(max_repair_failures=5, run_task_ceiling=999)
    res = await tool.execute({"goal": "fix the thing again"}, _ctx(parent, db, "spawn-1"))
    assert res.is_error and "already FAILED" in res.content
    # ...and nothing was spawned.
    async with db() as session:
        assert await session.get(Task, child_task_id(parent.id, "spawn-1")) is None


async def test_succeeded_children_never_trip_the_repair_breaker(db: Sessions) -> None:
    parent = await _enqueue(db, kind=TaskKind.PLAN, depth=0)
    await _add_children(db, parent, 8, TaskStatus.SUCCEEDED)
    tool = SpawnSubtaskTool(max_repair_failures=5, run_task_ceiling=999)
    res = await tool.execute({"goal": "another healthy micro-task"}, _ctx(parent, db, "spawn-2"))
    assert not res.is_error, res.content
    async with db() as session:
        assert await session.get(Task, child_task_id(parent.id, "spawn-2")) is not None


async def test_spawn_is_refused_at_the_run_task_ceiling(db: Sessions) -> None:
    parent = await _enqueue(db, kind=TaskKind.PLAN, depth=0)
    await _add_children(db, parent, 5, TaskStatus.SUCCEEDED)  # parent + 5 = 6 tasks
    tool = SpawnSubtaskTool(max_repair_failures=999, run_task_ceiling=6)
    res = await tool.execute({"goal": "one too many"}, _ctx(parent, db, "spawn-3"))
    assert res.is_error and "ceiling" in res.content


async def test_spawn_batch_honors_the_repair_breaker_too(db: Sessions) -> None:
    parent = await _enqueue(db, kind=TaskKind.PLAN, depth=0)
    await _add_children(db, parent, 5, TaskStatus.FAILED)
    tool = SpawnBatchTool(max_repair_failures=5, run_task_ceiling=999)
    res = await tool.execute(
        {"children": [{"goal": "fix a"}, {"goal": "fix b"}]}, _ctx(parent, db, "batch-1")
    )
    assert res.is_error and "already FAILED" in res.content


async def test_a_healthy_leader_below_the_ceilings_still_spawns(db: Sessions) -> None:
    parent = await _enqueue(db, kind=TaskKind.PLAN, depth=0)
    await _add_children(db, parent, 2, TaskStatus.FAILED)  # under the repair ceiling
    tool = SpawnSubtaskTool(max_repair_failures=5, run_task_ceiling=50)
    res = await tool.execute({"goal": "legitimate work"}, _ctx(parent, db, "spawn-ok"))
    assert not res.is_error, res.content
