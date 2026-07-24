"""Per-run USD budget: cost is stored once on the terminal transition, the run's
spend sums across the tree, the leader is refused more spawns once it's exhausted,
and an individual task halts GRACEFULLY (cleanly, at a step boundary) — never a
mid-tool kill or a retryable FAIL."""

from __future__ import annotations

import sqlalchemy as sa
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.models import DEFAULT_WORKSPACE_ID, Task, TaskKind, TaskStatus
from gantry.runtime.loop import run_agent_task
from gantry.runtime.tools import ToolRegistry
from gantry.worker.tools.orchestration import SpawnBatchTool, SpawnSubtaskTool

from .fakes import RecordingTool, response_with_tool_call
from .test_agent_loop import enqueue_agent_task
from .test_orchestration import _children_of, ctx_for, make_planner

Sessions = async_sessionmaker[AsyncSession]


async def test_complete_stores_cost_and_run_spend_sums_the_tree(db: Sessions) -> None:
    root = await enqueue_agent_task(db)
    async with session_scope(db) as session:
        claimed = await queue.claim(session, worker_id="w")
    assert claimed is not None
    async with session_scope(db) as session:
        ok = await queue.complete(session, task_id=root.id, worker_id="w", attempt=1, cost_usd=1.25)
        assert ok
        # A second (zombie) complete can't re-add — exactly-once, fenced.
        again = await queue.complete(
            session, task_id=root.id, worker_id="w", attempt=1, cost_usd=99
        )
        assert not again
    async with session_scope(db) as session:
        assert await queue.run_spend_usd(session, root.id) == 1.25


async def test_run_spend_covers_children(db: Sessions) -> None:
    parent = await make_planner(db)
    async with session_scope(db) as session:
        for _ in range(2):
            child = await queue.enqueue(
                session,
                workspace_id=DEFAULT_WORKSPACE_ID,
                kind=TaskKind.EXECUTE,
                payload={},
                parent=parent,
            )
            await session.execute(
                sa.update(Task)
                .where(Task.id == child.id)
                .values(status=TaskStatus.SUCCEEDED, cost_usd=2.0)
            )
    async with session_scope(db) as session:
        assert await queue.run_spend_usd(session, parent.id) == 4.0


async def test_spawn_is_refused_once_the_run_budget_is_exhausted(db: Sessions) -> None:
    # A leader whose already-finished child spent past the $1 budget.
    parent = await make_planner(db)
    async with session_scope(db) as session:
        await session.execute(
            sa.update(Task).where(Task.id == parent.id).values(payload={"budget_usd": 1.0})
        )
        child = await queue.enqueue(
            session,
            workspace_id=DEFAULT_WORKSPACE_ID,
            kind=TaskKind.EXECUTE,
            payload={},
            parent=parent,
        )
        await session.execute(
            sa.update(Task)
            .where(Task.id == child.id)
            .values(status=TaskStatus.SUCCEEDED, cost_usd=1.5)
        )
    # Both spawn tools refuse and steer the leader to converge — no new children.
    one = await SpawnSubtaskTool().execute({"goal": "more"}, ctx_for(parent, db, "c1"))
    assert (
        one.is_error and "budget exhausted" in one.content and "integrate and land" in one.content
    )
    batch = await SpawnBatchTool().execute(
        {"children": [{"goal": "more"}]}, ctx_for(parent, db, "c2")
    )
    assert batch.is_error and "budget exhausted" in batch.content
    existing = await _children_of(db, parent.id)
    assert [c.id for c in existing] == [child.id]  # nothing new spawned


async def test_task_halts_gracefully_when_its_spend_crosses_the_budget(db: Sessions) -> None:
    # A tiny budget: the task runs one step, then halts cleanly at the next
    # step boundary instead of continuing (or being killed mid-tool).
    task = await enqueue_agent_task(db, {"budget_usd": 1e-9})
    llm = _ToolThenMoreLLM()
    outcome = await run_agent_task(db, task, llm, ToolRegistry([RecordingTool()]))

    # It returned a clean outcome (no raise -> the worker marks it SUCCEEDED, not a
    # retryable FAIL) with a halt message and the accrued cost for the ledger.
    assert "Halted" in outcome.final_text and "budget" in outcome.final_text
    assert outcome.cost_usd > 0
    # Exactly one step ran before the next-boundary halt (never mid-tool).
    assert outcome.steps == 1


class _ToolThenMoreLLM:
    """Always calls the tool — so only the budget halt can end the loop."""

    async def complete(self, *, model, messages, tools=(), on_delta=None):  # type: ignore[no-untyped-def]
        n = sum(1 for m in messages if m.get("role") == "tool")
        return response_with_tool_call(f"c{n}", "increment", {"n": n})
