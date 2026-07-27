"""Milestone 2 acceptance: a leader -> sub-leader -> worker chain against real
Postgres and a real git remote.

This proves nested delegation composes end to end: the leaf worker writes a file
and pushes its branch; the SUB-LEADER integrates that branch and pushes a staging
branch; the top LEADER integrates the sub-leader's delivered branch and lands it
on main. The whole point is the delivery invariant a sub-leader depends on — a
task reports the branch it actually pushed (its HEAD), so a sub-leader hands its
integrated staging branch up to its parent, not its own empty task branch.
"""

from __future__ import annotations

import asyncio
from collections.abc import Sequence
from pathlib import Path
from typing import Any

import sqlalchemy as sa
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.models import DEFAULT_WORKSPACE_ID, Task, TaskKind, TaskStatus
from gantry.runtime.llm import DeltaSink, LLMResponse, Message, ToolSchema
from gantry.worker.service import Worker, WorkerConfig

from .fakes import final_response, response_with_tool_call
from .test_worker_git import git, origin  # noqa: F401  (fixture re-export)

Sessions = async_sessionmaker[AsyncSession]

GREETING = "MESSAGE = 'hello from the nested swarm'\n"


class NestedSwarmLLM:
    """Process-stateless leader / sub-leader / worker, routed by the task goal and
    its own durable tool history (so it survives every park, wake, and re-claim).

    - ``LEADER:``    spawn a sub-leader -> wait -> merge its branch -> land on main.
    - ``SUBLEADER:`` spawn a worker -> wait -> merge its branch (delivered up).
    - ``WORKER:``    write the file -> commit & push -> done.
    """

    async def complete(
        self,
        *,
        model: str,
        messages: list[Message],
        tools: Sequence[ToolSchema] = (),
        on_delta: DeltaSink | None = None,
    ) -> LLMResponse:
        goal = str(messages[1].get("content") or "")
        tool_contents = [str(m.get("content") or "") for m in messages if m.get("role") == "tool"]

        def seen(marker: str) -> bool:
            return any(marker in c for c in tool_contents)

        if goal.startswith("WORKER:"):
            n = len(tool_contents)
            if n == 0:
                return response_with_tool_call(
                    "w_write", "write_file", {"path": "greeting.py", "content": GREETING}
                )
            if n == 1:
                return response_with_tool_call("w_push", "git_commit_push", {"message": "greeting"})
            return final_response("WORKER DONE")

        # Both a leader and a sub-leader delegate; they differ only in whether they
        # land on main. Markers are read from THIS task's own history, so the two
        # never cross even though they share phrasing.
        if not seen("spawned subtask"):
            child = (
                {"goal": "SUBLEADER: build the greeting module", "role": "sub_leader"}
                if goal.startswith("LEADER:")
                else {"goal": "WORKER: create greeting.py"}
            )
            return response_with_tool_call("spawn_1", "spawn_subtask", child)
        if not seen('"children"'):
            return response_with_tool_call("wait_1", "wait_for_children", {})
        if not seen('"staging_branch"'):
            return response_with_tool_call("merge_1", "merge_child_branches", {})
        if goal.startswith("LEADER:") and not seen("landed "):
            return response_with_tool_call("land_1", "land_branch", {})
        return final_response("LEADER DONE" if goal.startswith("LEADER:") else "SUBLEADER DONE")


async def _status(db: Sessions, task_id: Any) -> TaskStatus:
    async with db() as session:
        status = await session.scalar(sa.select(Task.status).where(Task.id == task_id))
    assert status is not None
    return TaskStatus(status)


async def test_leader_sub_leader_worker_lands_on_main(
    db: Sessions,
    origin: Path,  # noqa: F811
    tmp_path: Path,
) -> None:
    async with session_scope(db) as session:
        leader = await queue.enqueue(
            session,
            workspace_id=DEFAULT_WORKSPACE_ID,
            kind=TaskKind.PLAN,
            payload={
                "goal": "LEADER: ship a greeting module",
                "repo_url": str(origin),
                "autonomous_leader": True,
            },
            max_attempts=20,
        )

    llm = NestedSwarmLLM()
    workers = [
        Worker(
            db,
            WorkerConfig(
                worker_id=f"nest-{i}",
                workspace_root=tmp_path / f"ws{i}",
                lease_seconds=30,
                poll_interval_seconds=0.05,
            ),
            llm,
        )
        for i in range(4)
    ]
    shutdown = asyncio.Event()
    runs = [asyncio.create_task(w.run(shutdown)) for w in workers]
    try:
        deadline = asyncio.get_running_loop().time() + 60
        while await _status(db, leader.id) is not TaskStatus.SUCCEEDED:
            assert asyncio.get_running_loop().time() < deadline, (
                f"leader never finished (status {await _status(db, leader.id)})"
            )
            await asyncio.sleep(0.1)
    finally:
        shutdown.set()
        await asyncio.gather(*runs)

    # The whole tree settled: leader (plan) -> sub-leader (plan) -> worker (execute).
    async with db() as session:
        tree = (await session.scalars(sa.select(Task).where(Task.root_task_id == leader.id))).all()
    by_kind = {t.kind for t in tree}
    assert by_kind == {TaskKind.PLAN, TaskKind.EXECUTE}
    sub = next(t for t in tree if t.id != leader.id and t.kind is TaskKind.PLAN)
    worker = next(t for t in tree if t.kind is TaskKind.EXECUTE)
    assert sub.parent_task_id == leader.id
    assert worker.parent_task_id == sub.id
    assert sub.payload["sub_leader"] is True
    assert all(t.status is TaskStatus.SUCCEEDED for t in tree)

    # The sub-leader delivered its INTEGRATED branch (a staging branch carrying the
    # worker's file), not its own empty task branch — the invariant nesting needs.
    delivered = str((sub.result or {}).get("branch") or "")
    assert delivered.startswith("gantry/staging-"), f"sub-leader delivered {delivered!r}"
    assert git("--git-dir", str(origin), "show", f"{delivered}:greeting.py") == GREETING.strip()

    # And the leader landed the whole chain on main: the greeting reached the
    # default branch through two levels of integration.
    assert git("--git-dir", str(origin), "show", "main:greeting.py") == GREETING.strip()
