"""Planner tools: fan out subtasks, then sleep until they settle.

``spawn_subtask`` is **exactly-once** across crashes: the child's task id is
derived deterministically from (parent id, tool call id), so a re-run of the
same call — normal crash recovery — converges on the already-spawned child
instead of duplicating it.

``wait_for_children`` is the dormancy switch. It reads the children's states:
all settled → returns the aggregated report; otherwise it raises
:class:`TaskParked` and the worker parks the planner (status
``waiting_children``, no lease, zero compute). The dangling ``tool_call``
checkpoint is the resume point — when a finishing child re-queues the parent,
any worker re-claims it and re-executes this call, which now returns the
report. Parking and resuming are the same machinery as crash recovery.
"""

from __future__ import annotations

import json
import uuid
from typing import Any, ClassVar, cast

import sqlalchemy as sa
from sqlalchemy.exc import IntegrityError
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.models import TERMINAL_STATUSES, Task, TaskKind, TaskStatus
from gantry.runtime.state import PLANNER_SYSTEM_PROMPT
from gantry.runtime.tools import (
    TaskParked,
    Tool,
    ToolContext,
    ToolIdempotency,
    ToolRegistry,
    ToolResult,
)
from gantry.teams import TeamNode, node_kind, node_payload_fields

Sessions = async_sessionmaker[AsyncSession]

#: Namespace for deriving deterministic child task ids.
_SPAWN_NAMESPACE = uuid.UUID("6a1a9f5e-0000-4000-8000-67616e747279")

#: Runaway-planner guard: hard cap on children per task.
DEFAULT_MAX_SUBTASKS = 32

#: Recursion guard: how deep the spawn tree may go (root is depth 0). Stops a
#: delegating agent from spawning delegating agents without bound.
MAX_SPAWN_DEPTH = 5

_RESULT_TEXT_CAP = 2000

#: Each park/wake cycle re-claims the planner (attempt += 1 — it's the fencing
#: token), so planners need generous max_attempts headroom.
DEFAULT_PLANNER_MAX_ATTEMPTS = 20


def child_task_id(parent_id: uuid.UUID, tool_call_id: str) -> uuid.UUID:
    return uuid.uuid5(_SPAWN_NAMESPACE, f"{parent_id}:{tool_call_id}")


#: Tokens that betray an invented/example repo URL rather than a real one.
#: Planners (especially smaller models) fabricate these when a goal mentions
#: git, which then fails the child at clone time.
_PLACEHOLDER_REPO_MARKERS = (
    "your-org",
    "your_org",
    "your-username",
    "yourusername",
    "example.com",
    "example.org",
    "my-org",
    "myorg",
    "org-name",
    "placeholder",
    "<",
    ">",
)


def _looks_like_placeholder(url: str) -> bool:
    low = url.lower()
    return any(marker in low for marker in _PLACEHOLDER_REPO_MARKERS)


def _inherit_parent_context(payload: dict[str, Any], parent_payload: dict[str, Any]) -> None:
    """Fill a child's unset execution context from the parent (planner).

    A delegated child works the SAME repo and — crucially — must use the SAME
    LLM credentials as the planner, or it falls back to the keyless server
    default and fails with an auth error. ``repo_url``/``base_branch`` inherit
    independently; ``provider_id`` and ``model`` inherit only as a matched pair
    (a model string is provider-specific) and only when the child pinned
    neither itself.
    """
    for key in ("repo_url", "base_branch"):
        if payload.get(key) is None and parent_payload.get(key) is not None:
            payload[key] = parent_payload[key]
    # A run-wide setting: auto-accept flows down to every descendant.
    if parent_payload.get("auto_approve") and payload.get("auto_approve") is None:
        payload["auto_approve"] = True
    if "provider_id" not in payload and "model" not in payload:
        for key in ("provider_id", "model"):
            if parent_payload.get(key) is not None:
                payload[key] = parent_payload[key]


def _sessions_of(ctx: ToolContext) -> Sessions:
    if ctx.sessions is None:
        raise RuntimeError("orchestration tools require ToolContext.sessions")
    return cast("Sessions", ctx.sessions)


class SpawnSubtaskTool(Tool):
    name = "spawn_subtask"
    description = (
        "Spawn one child task executed in parallel by another worker. Returns the "
        "child's task id immediately; results arrive later via wait_for_children. "
        "To run agents concurrently, call spawn_subtask several times in a row "
        "(e.g. launch all 5 coders at once) BEFORE calling wait_for_children — each "
        "call returns instantly and the children run at the same time. The child "
        "worker shares no context with you — its goal must be fully self-contained."
    )
    parameters: ClassVar[dict[str, Any]] = {
        "type": "object",
        "properties": {
            "goal": {"type": "string", "description": "Self-contained goal for the child"},
            "repo_url": {
                "type": "string",
                "description": (
                    "Only for an EXISTING repo the child should clone and push to. "
                    "Omit it for a brand-new/greenfield project — the child then starts "
                    "in an empty workspace and can `git init` there. Never invent a "
                    "placeholder URL (e.g. github.com/your-org/...): a repo that can't "
                    "be cloned fails the child immediately."
                ),
            },
            "base_branch": {"type": "string"},
            "model": {"type": "string", "description": "Override the child's LLM model"},
            "max_steps": {"type": "integer"},
            "priority": {"type": "integer", "description": "Higher runs earlier (default 0)"},
            "max_attempts": {"type": "integer"},
            "kind": {"type": "string", "enum": ["execute", "plan"]},
            "agent": {
                "type": "string",
                "description": (
                    "Name of one of your team's child agents (see 'Your team' in your "
                    "instructions). The child runs with that agent's own system prompt, "
                    "model, and permissions; `kind` is then derived from the agent."
                ),
            },
            "payload": {"type": "object", "description": "Extra payload fields passed verbatim"},
        },
        "required": ["goal"],
    }
    #: The deterministic child id makes re-runs converge, so IDEMPOTENT.
    idempotency = ToolIdempotency.IDEMPOTENT

    def __init__(
        self, max_subtasks: int = DEFAULT_MAX_SUBTASKS, team: TeamNode | None = None
    ) -> None:
        self._max_subtasks = max_subtasks
        self._team = team

    def _team_child(self, name: str) -> TeamNode | None:
        children = (self._team or {}).get("children") or []
        return next((c for c in children if c.get("name") == name), None)

    async def execute(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult:
        goal = str(arguments.get("goal") or "").strip()
        if not goal:
            return ToolResult("a non-empty goal is required", is_error=True)
        if ctx.tool_call_id is None:
            return ToolResult("spawn_subtask requires a tool call id", is_error=True)
        explicit_repo = arguments.get("repo_url")
        if explicit_repo and _looks_like_placeholder(str(explicit_repo)):
            return ToolResult(
                f"repo_url {explicit_repo!r} looks like a placeholder, not a real "
                "repository. Omit repo_url to reuse the team's repo (or to start in "
                "an empty workspace), or pass a real repo you can access.",
                is_error=True,
            )
        agent = str(arguments.get("agent") or "").strip() or None
        node: TeamNode | None = None
        if agent is not None:
            node = self._team_child(agent)
            if node is None:
                team_children = (self._team or {}).get("children") or []
                if team_children:
                    names = ", ".join(str(c.get("name")) for c in team_children)
                    return ToolResult(
                        f"unknown agent {agent!r}; available agents: {names}", is_error=True
                    )
                # A dynamic-swarm leader (no fixed team) named an agent it made up —
                # there is no roster to pick from, so drop the name and spawn a
                # generic worker from the self-contained goal instead of failing.
                agent = None
        sessions = _sessions_of(ctx)
        child_id = child_task_id(ctx.task_id, ctx.tool_call_id)

        async with session_scope(sessions) as session:
            existing = await session.get(Task, child_id)
            if existing is not None:  # crash-recovery re-run: already spawned
                return ToolResult(f"subtask already spawned: task {child_id}")

            parent = await session.get(Task, ctx.task_id)
            if parent is None:
                return ToolResult("parent task not found", is_error=True)
            child_count = (
                await session.scalar(
                    sa.select(sa.func.count())
                    .select_from(Task)
                    .where(Task.parent_task_id == parent.id)
                )
                or 0
            )
            if child_count >= self._max_subtasks:
                return ToolResult(
                    f"subtask cap reached ({self._max_subtasks}); integrate existing "
                    "children before spawning more",
                    is_error=True,
                )
            child_depth = int(parent.payload.get("depth") or 0) + 1
            if child_depth > MAX_SPAWN_DEPTH:
                return ToolResult(
                    f"spawn depth cap reached (max {MAX_SPAWN_DEPTH}); this agent is too "
                    "deep in the tree to spawn more children — do the work directly or "
                    "report back to your parent",
                    is_error=True,
                )

            payload: dict[str, Any] = dict(arguments.get("payload") or {})
            if node is not None:
                # Team spawn: the child's config comes from the parent's
                # immutable payload snapshot — deterministic across re-runs.
                payload.update(node_payload_fields(node))
            payload["goal"] = goal
            # Explicit tool args win over the profile snapshot. A blank string
            # is treated as "not provided" so it falls through to inheritance
            # rather than forcing an empty override.
            for key in ("repo_url", "base_branch", "model", "max_steps"):
                value = arguments.get(key)
                if value is not None and not (isinstance(value, str) and not value.strip()):
                    payload[key] = value
            _inherit_parent_context(payload, parent.payload)
            payload["depth"] = child_depth
            if node is not None:
                kind = node_kind(node)  # derived from the snapshot, not the argument
            else:
                kind = TaskKind(str(arguments.get("kind") or TaskKind.EXECUTE.value))
            if kind is TaskKind.PLAN:
                payload.setdefault("system_prompt", PLANNER_SYSTEM_PROMPT)
            default_attempts = DEFAULT_PLANNER_MAX_ATTEMPTS if kind is TaskKind.PLAN else 3
            try:
                child = await queue.enqueue(
                    session,
                    workspace_id=parent.workspace_id,
                    kind=kind,
                    payload=payload,
                    parent=parent,
                    priority=int(arguments.get("priority") or 0),
                    max_attempts=int(arguments.get("max_attempts") or default_attempts),
                    task_id=child_id,
                )
            except IntegrityError:  # lost a rare race with our own zombie
                return ToolResult(f"subtask already spawned: task {child_id}")
        label = f" as agent {agent!r}" if agent else ""
        return ToolResult(f"spawned subtask {child.id} ({kind.value}){label}: {goal}")


class WaitForChildrenTool(Tool):
    name = "wait_for_children"
    description = (
        "Sleep (at zero compute cost) until every spawned subtask has finished, "
        "then receive a report of each child's status and result. Spawn ALL the "
        "children you need first (they run concurrently), then call this ONCE to "
        "wait for the whole batch — do not spawn one, wait, spawn the next. To act "
        "on children as they finish individually instead, poll agent_status. Never "
        "poll in a busy loop."
    )
    parameters: ClassVar[dict[str, Any]] = {"type": "object", "properties": {}}
    #: Re-running after a crash just re-checks state — naturally idempotent.
    idempotency = ToolIdempotency.IDEMPOTENT

    async def execute(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult:
        sessions = _sessions_of(ctx)
        async with session_scope(sessions) as session:
            children = (
                await session.scalars(
                    sa.select(Task)
                    .where(Task.parent_task_id == ctx.task_id)
                    .order_by(Task.created_at, Task.id)
                )
            ).all()
        if not children:
            return ToolResult("no subtasks have been spawned; nothing to wait for")
        unsettled = [c for c in children if c.status not in TERMINAL_STATUSES]
        if unsettled:
            raise TaskParked(TaskStatus.WAITING_CHILDREN.value)
        return ToolResult(_children_report(list(children)))


def _children_report(children: list[Task]) -> str:
    report = []
    for child in children:
        result = child.result or {}
        entry: dict[str, Any] = {
            "task_id": str(child.id),
            "goal": child.payload.get("goal"),
            "status": child.status.value,
        }
        if child.status is TaskStatus.SUCCEEDED:
            entry["final_text"] = str(result.get("final_text") or "")[:_RESULT_TEXT_CAP]
            if result.get("branch"):
                entry["branch"] = result["branch"]
        elif child.last_error:
            entry["error"] = child.last_error[:_RESULT_TEXT_CAP]
        report.append(entry)
    counts = {
        status.value: n
        for status in TERMINAL_STATUSES
        if (n := sum(1 for c in children if c.status is status))
    }
    return json.dumps({"summary": counts, "children": report}, indent=2)


class AgentStatusTool(Tool):
    name = "agent_status"
    parallel_safe = True
    description = (
        "Check the current status (and result, if finished) of one child agent you "
        "spawned, by its task id. Use this to poll a specific child; to sleep until "
        "ALL children finish, use wait_for_children instead."
    )
    parameters: ClassVar[dict[str, Any]] = {
        "type": "object",
        "properties": {"task_id": {"type": "string", "description": "The child's task id"}},
        "required": ["task_id"],
    }
    idempotency = ToolIdempotency.IDEMPOTENT

    async def execute(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult:
        child = await _lookup_child(arguments, ctx)
        if isinstance(child, ToolResult):
            return child
        return ToolResult(json.dumps(_child_entry(child), indent=2))


class AgentTerminateTool(Tool):
    name = "agent_terminate"
    description = (
        "Stop one child agent you spawned, by its task id — for a runaway or no-longer-"
        "needed child. Already-finished children are left as they are."
    )
    parameters: ClassVar[dict[str, Any]] = {
        "type": "object",
        "properties": {"task_id": {"type": "string", "description": "The child's task id"}},
        "required": ["task_id"],
    }
    #: Cancelling an already-cancelled/finished task is a no-op — safe to re-run.
    idempotency = ToolIdempotency.IDEMPOTENT

    async def execute(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult:
        child = await _lookup_child(arguments, ctx)
        if isinstance(child, ToolResult):
            return child
        sessions = _sessions_of(ctx)
        async with session_scope(sessions) as session:
            result = await queue.cancel(session, task_id=child.id)
        if result is None:
            return ToolResult(f"child {child.id} is already {child.status.value}; not stopped")
        return ToolResult(f"requested stop of child {child.id} ({result.task.status.value})")


async def _lookup_child(arguments: dict[str, Any], ctx: ToolContext) -> Task | ToolResult:
    """Resolve a child task id argument, enforcing it is a direct child."""
    raw = str(arguments.get("task_id") or "").strip()
    try:
        child_id = uuid.UUID(raw)
    except ValueError:
        return ToolResult(
            f"invalid task id {raw!r}: that is not a task id. Pass the exact id (a "
            "UUID) that spawn_subtask returned when you launched the child — do not "
            "make up a name. To wait for the whole batch at once, use wait_for_children.",
            is_error=True,
        )
    sessions = _sessions_of(ctx)
    async with session_scope(sessions) as session:
        child = await session.get(Task, child_id)
    if child is None or child.parent_task_id != ctx.task_id:
        return ToolResult(
            f"task {raw} is not one of your children (you can only inspect agents you spawned)",
            is_error=True,
        )
    return child


def _child_entry(child: Task) -> dict[str, Any]:
    result = child.result or {}
    entry: dict[str, Any] = {
        "task_id": str(child.id),
        "goal": child.payload.get("goal"),
        "status": child.status.value,
    }
    if child.status is TaskStatus.SUCCEEDED:
        entry["final_text"] = str(result.get("final_text") or "")[:_RESULT_TEXT_CAP]
    elif child.last_error:
        entry["error"] = child.last_error[:_RESULT_TEXT_CAP]
    return entry


def orchestration_tools(
    max_subtasks: int = DEFAULT_MAX_SUBTASKS, team: TeamNode | None = None
) -> list[Tool]:
    """The delegation toolset: spawn/wait plus per-child status/terminate."""
    return [
        SpawnSubtaskTool(max_subtasks, team=team),
        WaitForChildrenTool(),
        AgentStatusTool(),
        AgentTerminateTool(),
    ]


def build_planner_registry(
    max_subtasks: int = DEFAULT_MAX_SUBTASKS, team: TeamNode | None = None
) -> ToolRegistry:
    from gantry.worker.tools.ask import AskUserTool

    registry = ToolRegistry(orchestration_tools(max_subtasks, team=team))
    registry.register(AskUserTool())
    return registry
