"""The MVP acceptance test — the MASTERPLAN sentence, executed literally:

  "Given a GitHub repo and a goal, Gantry plans, fans out sandboxed workers,
   survives worker kills, pauses for approval on destructive actions, and
   delivers pushed feature branches — all observable live in the browser."

One test composes every subsystem: a planner fans out two repo-backed
children; one child's first claim dies (lease reaped — the kill path); the
other child hits the approval gate and parks until a human approves; both
push real branches to the origin; the planner integrates. Every step is in
the event log — the substrate the browser UI streams from.
"""

from __future__ import annotations

import asyncio
import json
from pathlib import Path

import sqlalchemy as sa
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.events import read_events
from gantry.core.models import DEFAULT_WORKSPACE_ID, Task, TaskKind, TaskStatus
from gantry.worker.service import Worker, WorkerConfig

from .fakes import final_response, multi_tool_response, response_with_tool_call
from .test_worker_git import git, origin  # noqa: F401  (fixture re-export)

Sessions = async_sessionmaker[AsyncSession]

REPO_ROOT = Path(__file__).resolve().parent.parent


class MvpLLM:
    """Stateless scripts for the planner and both children, routed by goal."""

    def __init__(self, origin_url: str) -> None:
        self._origin = origin_url

    async def complete(self, *, model, messages, tools=()):  # type: ignore[no-untyped-def]
        goal = str(messages[1].get("content") or "")
        tool_msgs = [str(m.get("content") or "") for m in messages if m.get("role") == "tool"]
        if goal.startswith("PLAN:"):
            return self._plan(tool_msgs)
        if "alpha" in goal:
            return self._alpha(tool_msgs)
        return self._beta(tool_msgs)

    def _plan(self, tool_msgs: list[str]):  # type: ignore[no-untyped-def]
        reports = [c for c in tool_msgs if '"children"' in c]
        if not tool_msgs:
            return multi_tool_response(
                (
                    "s1",
                    "spawn_subtask",
                    {"goal": "Write alpha.txt and deliver the branch", "repo_url": self._origin},
                ),
                (
                    "s2",
                    "spawn_subtask",
                    {
                        "goal": "Clean the scratch dir, write beta.txt, deliver the branch",
                        "repo_url": self._origin,
                    },
                ),
            )
        if not reports:
            return response_with_tool_call("w1", "wait_for_children", {})
        children = json.loads(reports[-1])["children"]
        branches = sorted(str(c.get("branch")) for c in children)
        return final_response(f"shipped on branches: {', '.join(branches)}")

    def _alpha(self, tool_msgs: list[str]):  # type: ignore[no-untyped-def]
        if len(tool_msgs) == 0:
            return response_with_tool_call(
                "a1", "write_file", {"path": "alpha.txt", "content": "alpha survives kills\n"}
            )
        if len(tool_msgs) == 1:
            return response_with_tool_call(
                "a2", "git_commit_push", {"message": "feat(alpha): add alpha.txt"}
            )
        return final_response("alpha delivered")

    def _beta(self, tool_msgs: list[str]):  # type: ignore[no-untyped-def]
        if len(tool_msgs) == 0:
            return response_with_tool_call("b1", "bash", {"command": "rm -rf scratch"})
        if len(tool_msgs) == 1:
            return response_with_tool_call(
                "b2", "write_file", {"path": "beta.txt", "content": "beta approved by human\n"}
            )
        if len(tool_msgs) == 2:
            return response_with_tool_call(
                "b3", "git_commit_push", {"message": "feat(beta): add beta.txt"}
            )
        return final_response("beta delivered")


async def get_task(db: Sessions, task_id) -> Task:  # type: ignore[no-untyped-def]
    async with db() as session:
        task = await session.get(Task, task_id)
    assert task is not None
    return task


async def test_mvp_acceptance(db: Sessions, origin: Path, tmp_path: Path) -> None:  # noqa: F811
    llm = MvpLLM(str(origin))

    # Given a repo and a goal: a planner task.
    async with session_scope(db) as session:
        planner = await queue.enqueue(
            session,
            workspace_id=DEFAULT_WORKSPACE_ID,
            kind=TaskKind.PLAN,
            payload={"goal": "PLAN: ship alpha and beta", "model": "fake/mvp"},
            max_attempts=20,
        )

    # Gantry plans: one worker runs the planner until it parks on its children.
    seed_worker = Worker(
        db,
        WorkerConfig(worker_id="seed", workspace_root=tmp_path / "seed", lease_seconds=30),
        llm,
    )
    async with session_scope(db) as session:
        claimed = await queue.claim(session, worker_id="seed", lease_seconds=30)
    assert claimed is not None and claimed.id == planner.id
    await seed_worker.process(claimed)
    assert (await get_task(db, planner.id)).status is TaskStatus.WAITING_CHILDREN

    async with db() as session:
        children = (
            await session.scalars(sa.select(Task).where(Task.parent_task_id == planner.id))
        ).all()
    assert len(children) == 2
    alpha = next(c for c in children if "alpha" in str(c.payload["goal"]))
    beta = next(c for c in children if "beta" in str(c.payload["goal"]))

    # Survives worker kills: alpha's first claimer dies instantly (its lease
    # expires unheartbeaten); the reaper hands it to the fleet.
    async with session_scope(db) as session:
        doomed = await queue.claim(session, worker_id="doomed", lease_seconds=0.05)
    assert doomed is not None
    await asyncio.sleep(0.2)

    # Fan out sandboxed workers: a fleet of three picks up everything else.
    fleet = [
        Worker(
            db,
            WorkerConfig(
                worker_id=f"fleet-{i}",
                workspace_root=tmp_path / f"w{i}",
                lease_seconds=30,
                poll_interval_seconds=0.05,
                skills_root=REPO_ROOT / "skills",
            ),
            llm,
        )
        for i in range(3)
    ]
    shutdown = asyncio.Event()
    runs = [asyncio.create_task(w.run(shutdown)) for w in fleet]
    approved = False
    try:
        deadline = asyncio.get_running_loop().time() + 60
        while (await get_task(db, planner.id)).status is not TaskStatus.SUCCEEDED:
            assert asyncio.get_running_loop().time() < deadline, "MVP run never finished"
            # The control plane's jobs, performed by the test loop: reap dead
            # workers' leases and approve the gated destructive command.
            async with session_scope(db) as session:
                await queue.reap_expired(session)
            if not approved and (await get_task(db, beta.id)).status is TaskStatus.WAITING_APPROVAL:
                async with session_scope(db) as session:
                    outcome, _ = await queue.resolve_approval(
                        session, task_id=beta.id, tool_call_id="b1", approved=True, comment="ok"
                    )
                assert outcome == "resolved"
                approved = True
            await asyncio.sleep(0.1)
    finally:
        shutdown.set()
        await asyncio.gather(*runs)

    # Pauses for approval on destructive actions: beta really parked and resumed.
    assert approved, "the destructive command never reached the approval gate"
    beta_events = [e.event_type.value for e in await _events(db, beta.id)]
    for expected in ("approval_requested", "task_parked", "approval_resolved", "task_resumed"):
        assert expected in beta_events

    # Survives worker kills: alpha finished on a later attempt after the reap.
    alpha_final = await get_task(db, alpha.id)
    assert alpha_final.status is TaskStatus.SUCCEEDED
    assert alpha_final.attempt >= 2
    alpha_events = [e.event_type.value for e in await _events(db, alpha.id)]
    assert "task_lease_expired" in alpha_events

    # Delivers pushed feature branches: both children's work is on the origin.
    for task_row, filename, content in (
        (alpha_final, "alpha.txt", "alpha survives kills"),
        (await get_task(db, beta.id), "beta.txt", "beta approved by human"),
    ):
        assert task_row.result is not None
        branch = task_row.result["branch"]
        assert branch and branch.startswith("gantry/task-")
        assert git("--git-dir", str(origin), "show", f"{branch}:{filename}").strip() == content

    # Skills attached themselves by goal keywords and are pinned in the log.
    assert "skill_injected" in alpha_events

    # The planner integrated the children's results.
    planner_final = await get_task(db, planner.id)
    assert planner_final.result is not None
    assert "shipped on branches: " in planner_final.result["final_text"]

    # "Observable live in the browser": the full story is one ordered event
    # log per task — the exact stream the UI's websockets replay and tail.
    assert beta_events.index("approval_requested") < beta_events.index("approval_resolved")
    assert beta_events[-1] == "task_succeeded"


async def _events(db: Sessions, task_id):  # type: ignore[no-untyped-def]
    async with session_scope(db) as session:
        return await read_events(session, task_id)
