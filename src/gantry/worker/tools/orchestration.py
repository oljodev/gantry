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

from gantry.attachments.snapshot import PAYLOAD_KEY as ATTACHMENTS_KEY
from gantry.attachments.snapshot import inheritable
from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.events import append_event, read_events
from gantry.core.models import TERMINAL_STATUSES, EventType, Task, TaskKind, TaskStatus
from gantry.prompts import PLANNER_SYSTEM_PROMPT
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

#: Repair-wave breaker: refuse to spawn once this many of a parent's DIRECT children
#: have terminally FAILED (a leader stuck re-spawning fixers must converge or report).
DEFAULT_MAX_REPAIR_FAILURES = 5
#: Structural backstop: refuse once the whole run tree holds this many tasks.
DEFAULT_RUN_TASK_CEILING = 50

#: Recursion guard: how deep the spawn tree may go (root is depth 0). Stops a
#: delegating agent from spawning delegating agents without bound.
MAX_SPAWN_DEPTH = 5

_RESULT_TEXT_CAP = 2000

#: The batch report (wait_for_children) is read straight into the leader's context,
#: so it must stay small even with ~100 children — otherwise it alone forces a leader
#: compaction (2000 chars x 100 ~= 75K tokens). A succeeded child is compressed to its
#: BRANCH + a short closing tail (the leader integrates by branch, not by re-reading
#: each worker's prose), while a FAILED child keeps its full error (the actionable
#: signal). The whole report is then hard-bounded so one wait can't blow the context.
_SUCCESS_TAIL_CHARS = 240
_ERROR_CHARS = 6000
_REPORT_CHAR_CAP = 48_000

#: Each park/wake cycle re-claims the planner (attempt += 1 — it's the fencing
#: token), so planners need generous max_attempts headroom.
DEFAULT_PLANNER_MAX_ATTEMPTS = 20


def child_task_id(parent_id: uuid.UUID, tool_call_id: str) -> uuid.UUID:
    return uuid.uuid5(_SPAWN_NAMESPACE, f"{parent_id}:{tool_call_id}")


def batch_child_id(parent_id: uuid.UUID, tool_call_id: str, index: int) -> uuid.UUID:
    """Deterministic id for the ``index``-th child of a spawn_batch call. NESTED
    off the single-child id (used as the namespace) so a batch element id can never
    collide with a spawn_subtask id — keeping spawns exactly-once across crash
    replay for both tools."""
    return uuid.uuid5(child_task_id(parent_id, tool_call_id), str(index))


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


def _is_unclonable_local_repo(url: str) -> bool:
    """True for a repo_url no isolated worker could clone: a ``file://`` URL, a bare
    local filesystem path, or a loopback remote.

    A dynamic leader that means "the repo I'm working in" sometimes fabricates one
    of these (e.g. ``file:///app/.git``, a hallucinated container path) instead of
    omitting repo_url. But each child runs in its OWN fresh workspace with no access
    to another task's checkout, so such a URL always fails at clone time. The caller
    (``_child_payload``) treats it like a blank repo_url — dropped so the child
    inherits the parent's real remote — but only when the parent HAS one to inherit,
    so an explicit local repo that is a child's only source is still honored.
    """
    low = url.strip().lower()
    if not low:
        return False
    # file:// URLs and bare local filesystem paths (/app/.git, ./x, ~/x, C:\...).
    if low.startswith(("file:", "/", "~", ".")) or low[1:3] == ":\\":
        return True
    # A loopback host over any scheme (http://localhost/…, ssh://127.0.0.1/…) or the
    # scheme-less scp form (localhost:path) — never reachable from another worker.
    return any(
        f"//{h}" in low or low.startswith((f"{h}:", f"{h}/"))
        for h in ("localhost", "127.0.0.1", "0.0.0.0", "::1")
    )


def _inherit_parent_context(payload: dict[str, Any], parent_payload: dict[str, Any]) -> None:
    """Fill a child's unset execution context from the parent (planner).

    A delegated child works the SAME repo and — crucially — must use the SAME
    LLM credentials as the planner, or it falls back to the keyless server
    default and fails with an auth error. So ``provider_id`` (the account/gateway
    key) inherits INDEPENDENTLY, like ``repo_url``/``base_branch``: a leader on an
    OpenRouter key can spawn a child on a cheaper model over that same key. Only
    ``model`` is conditional — a child that pinned its own model keeps it and runs
    it on the inherited provider, which for a multi-model gateway is one key
    across every model.
    """
    for key in ("repo_url", "base_branch", "provider_id", "budget_usd"):
        if payload.get(key) is None and parent_payload.get(key) is not None:
            payload[key] = parent_payload[key]
    # A run-wide setting: auto-accept flows down to every descendant.
    if parent_payload.get("auto_approve") and payload.get("auto_approve") is None:
        payload["auto_approve"] = True
    # An autonomous swarm is non-interactive top to bottom: the flag flows from an
    # autonomous leader (or any already-non-interactive parent) to every descendant,
    # so no spawned worker can park the run on ask_user either.
    if (
        parent_payload.get("non_interactive") or parent_payload.get("autonomous_leader")
    ) and payload.get("non_interactive") is None:
        payload["non_interactive"] = True
    # The child's own model (e.g. a cheap one for mechanical work) wins; only
    # borrow the parent's when the child pinned none.
    if payload.get("model") is None and parent_payload.get("model") is not None:
        payload["model"] = parent_payload["model"]
    # The user's attached files are part of the SPEC, so they flow down to every
    # worker that has to build against them — but only the durable snapshot: the
    # parent's model-specific resolution (its base64 image, its transcript) is
    # stripped so each child re-resolves against ITS OWN model. That is what makes
    # a vision leader delegating to a text-only coder work: the coder gets the
    # cached transcription, not an image block its provider would reject.
    if payload.get(ATTACHMENTS_KEY) is None and parent_payload.get(ATTACHMENTS_KEY):
        payload[ATTACHMENTS_KEY] = inheritable(parent_payload[ATTACHMENTS_KEY])


def _sessions_of(ctx: ToolContext) -> Sessions:
    if ctx.sessions is None:
        raise RuntimeError("orchestration tools require ToolContext.sessions")
    return cast("Sessions", ctx.sessions)


#: The per-child spec — shared by spawn_subtask (one) and spawn_batch (an array).
_CHILD_SPEC_PROPERTIES: dict[str, Any] = {
    "goal": {"type": "string", "description": "Self-contained goal for the child"},
    "repo_url": {
        "type": "string",
        "description": (
            "Only for an EXISTING repo the child should clone and push to. To make a "
            "child work on the SAME repo you're in, just OMIT this — it inherits yours; "
            "never pass a local path or file:// URL to your own checkout, as the child "
            "runs in a separate workspace and can't reach it. Omit it for a brand-new/"
            "greenfield project too — the child then starts in an empty workspace and "
            "can `git init` there. Never invent a placeholder URL (e.g. "
            "github.com/your-org/...): a repo that can't be cloned fails the child."
        ),
    },
    "base_branch": {"type": "string"},
    "model": {
        "type": "string",
        "description": (
            "The model this child runs on, as a provider slug (e.g. "
            "'deepseek/deepseek-chat'). Set it to match the task's difficulty so you "
            "don't burn an expensive model on cheap work: use a fast, cheap model for "
            "mechanical tasks (splitting files, reading, edits, QA testing) and reserve "
            "a strong reasoning model (e.g. 'deepseek/deepseek-r1') ONLY for hard "
            "algorithm design or deep debugging. Omit it to inherit your own model. It "
            "runs on your provider key, so pass a slug that key can serve."
        ),
    },
    "max_steps": {"type": "integer"},
    "priority": {"type": "integer", "description": "Higher runs earlier (default 0)"},
    "max_attempts": {"type": "integer"},
    "role": {
        "type": "string",
        "enum": ["worker", "sub_leader"],
        "description": (
            "'worker' (the default) does the change itself in one leaf task — use it "
            "for almost everything. 'sub_leader' spawns a delegating SUB-LEADER that "
            "runs the whole swarm workflow one level down: it surveys its sub-scope, "
            "fans out to its OWN 2-10 workers, integrates their branches, and pushes a "
            "single combined branch back up to you. Reach for it only when one slice is "
            "itself a large subsystem that needs its own decomposition, so no single "
            "leader has to fan out too wide."
        ),
    },
    "title": {
        "type": "string",
        "description": (
            "A tight ~3-word label for this task shown as its trace-tree node (e.g. "
            "'Refactor board.py', 'Add pieces module'). Omit it and one is derived from "
            "the goal."
        ),
    },
    "kind": {
        "type": "string",
        "enum": ["execute", "plan"],
        "description": (
            "Low-level task kind; prefer `role` instead. 'execute' is a worker, 'plan' "
            "is a sub-leader. `role` and a team `agent` both override this."
        ),
    },
    "agent": {
        "type": "string",
        "description": (
            "Name of one of your team's child agents (see 'Your team' in your "
            "instructions). The child runs with that agent's own system prompt, model, "
            "and permissions; `kind` is then derived from the agent."
        ),
    },
    "payload": {"type": "object", "description": "Extra payload fields passed verbatim"},
}


def _spec_error(spec: dict[str, Any]) -> str | None:
    """A well-formedness error for one child spec, or None."""
    if not str(spec.get("goal") or "").strip():
        return "a non-empty goal is required"
    repo = spec.get("repo_url")
    if repo and _looks_like_placeholder(str(repo)):
        return (
            f"repo_url {repo!r} looks like a placeholder, not a real repository. Omit "
            "repo_url to reuse the team's repo (or to start in an empty workspace), or "
            "pass a real repo you can access."
        )
    return None


async def _child_ids_of(session: AsyncSession, parent_id: uuid.UUID) -> set[uuid.UUID]:
    rows = await session.scalars(sa.select(Task.id).where(Task.parent_task_id == parent_id))
    return set(rows)


class _SpawnBase(Tool):
    """Shared machinery for spawn_subtask (one child) and spawn_batch (many): agent
    resolution, per-child payload build, and enqueue — all reused so the two tools
    build byte-identical child payloads."""

    #: The deterministic child id makes re-runs converge, so IDEMPOTENT.
    idempotency = ToolIdempotency.IDEMPOTENT

    def __init__(
        self,
        max_subtasks: int = DEFAULT_MAX_SUBTASKS,
        team: TeamNode | None = None,
        *,
        max_repair_failures: int = DEFAULT_MAX_REPAIR_FAILURES,
        run_task_ceiling: int = DEFAULT_RUN_TASK_CEILING,
    ) -> None:
        self._max_subtasks = max_subtasks
        self._team = team
        self._max_repair_failures = max_repair_failures
        self._run_task_ceiling = run_task_ceiling

    def _team_child(self, name: str) -> TeamNode | None:
        children = (self._team or {}).get("children") or []
        return next((c for c in children if c.get("name") == name), None)

    async def _swarm_health_block(self, session: AsyncSession, parent: Task) -> str | None:
        """Two hard spawn breakers beyond the per-parent subtask cap, both derived from
        committed rows (so a crash-replay makes the identical decision):

        - **Repair-wave**: once ``max_repair_failures`` of this parent's DIRECT children
          have terminally FAILED, refuse — a leader spawning fixer wave after fixer wave
          is funding an approach that keeps failing; it must integrate/land whatever
          already works or abort and report, not keep spending.
        - **Run capacity**: once the whole tree holds ``run_task_ceiling`` tasks, refuse
          — a structural bound on total fan-out regardless of nesting depth.

        ``None`` means there is headroom to spawn.
        """
        failed = await queue.failed_child_count(session, parent.id)
        if failed >= self._max_repair_failures:
            return (
                f"{failed} of your child tasks have already FAILED. Stop spawning fixers — "
                "the same approach keeps failing. Integrate and land whatever already "
                "works, or abort and report the failure with what you learned. Do NOT "
                "spawn more."
            )
        total = await queue.run_task_count(session, parent.root_task_id)
        if total >= self._run_task_ceiling:
            return (
                f"this run already has {total} tasks (ceiling {self._run_task_ceiling}). "
                "Do NOT spawn more — integrate and land what exists, then finish."
            )
        return None

    async def _budget_exhausted(self, session: AsyncSession, parent: Task) -> str | None:
        """The run-level GRACEFUL brake: if the run's settled spend crossed its
        budget, refuse to spawn MORE work (the leader then converges — integrate,
        land, finish — rather than fanning out). ``None`` means there is headroom."""
        budget = parent.payload.get("budget_usd")
        if budget is None:
            return None
        spent = await queue.run_spend_usd(session, parent.root_task_id)
        if spent >= float(budget):
            return (
                f"run budget exhausted (${spent:.2f} of ${float(budget):.2f} spent). Do NOT "
                "spawn more work — integrate and land what you already have, then finish."
            )
        return None

    def _resolve_node(
        self, spec: dict[str, Any]
    ) -> tuple[str | None, TeamNode | None] | ToolResult:
        """Resolve the optional `agent` name to a team node. Returns (agent, node),
        or an error ToolResult if a named agent isn't in a non-empty team roster."""
        agent = str(spec.get("agent") or "").strip() or None
        if agent is None:
            return None, None
        node = self._team_child(agent)
        if node is not None:
            return agent, node
        team_children = (self._team or {}).get("children") or []
        if team_children:
            names = ", ".join(str(c.get("name")) for c in team_children)
            return ToolResult(f"unknown agent {agent!r}; available agents: {names}", is_error=True)
        # A dynamic-swarm leader (no fixed team) named an agent it made up — there is
        # no roster to pick from, so drop the name and spawn a generic worker.
        return None, None

    def _child_payload(
        self, spec: dict[str, Any], parent: Task, node: TeamNode | None, child_depth: int
    ) -> tuple[dict[str, Any], TaskKind]:
        payload: dict[str, Any] = dict(spec.get("payload") or {})
        if node is not None:
            # Team spawn: the child's config comes from the parent's immutable
            # payload snapshot — deterministic across re-runs.
            payload.update(node_payload_fields(node))
        payload["goal"] = str(spec.get("goal") or "").strip()
        title = str(spec.get("title") or "").strip()
        if title:
            payload["title"] = title
        # Explicit tool args win over the profile snapshot. A blank string is treated
        # as "not provided" so it falls through to inheritance — as is a local/
        # self-referential repo_url (file://, a bare path) WHEN the parent has a real
        # repo to inherit: a leader working in repo X sometimes fabricates a local path
        # to "the repo I'm in" (e.g. file:///app/.git) that no isolated child can clone,
        # so the child inherits X instead of failing at clone time. (With no parent
        # repo, an explicit local path is the child's only repo, so it is kept.)
        parent_has_repo = bool(parent.payload.get("repo_url"))
        for key in ("repo_url", "base_branch", "model", "max_steps"):
            value = spec.get(key)
            if value is None or (isinstance(value, str) and not value.strip()):
                continue
            if (
                key == "repo_url"
                and parent_has_repo
                and isinstance(value, str)
                and _is_unclonable_local_repo(value)
            ):
                continue
            payload[key] = value
        _inherit_parent_context(payload, parent.payload)
        payload["depth"] = child_depth
        # `role` is the friendly knob a leader reaches for; `kind` is the low-level
        # value it maps to. A team `agent` node's kind wins over both (it comes from
        # the immutable profile snapshot). role='sub_leader' -> a delegating plan task.
        if node is not None:
            kind = node_kind(node)
        elif str(spec.get("role") or "").strip() == "sub_leader":
            kind = TaskKind.PLAN
        else:
            kind = TaskKind(str(spec.get("kind") or TaskKind.EXECUTE.value))
        if kind is TaskKind.PLAN:
            # A spawned plan task is a SUB-leader (it has a parent); mark it so the UI
            # can label it distinctly and the delegation role is explicit in the log.
            payload.setdefault("system_prompt", PLANNER_SYSTEM_PROMPT)
            payload["sub_leader"] = True
        return payload, kind

    async def _enqueue_child(
        self,
        session: AsyncSession,
        spec: dict[str, Any],
        parent: Task,
        node: TeamNode | None,
        child_id: uuid.UUID,
        child_depth: int,
    ) -> Task:
        payload, kind = self._child_payload(spec, parent, node, child_depth)
        default_attempts = DEFAULT_PLANNER_MAX_ATTEMPTS if kind is TaskKind.PLAN else 3
        return await queue.enqueue(
            session,
            workspace_id=parent.workspace_id,
            kind=kind,
            payload=payload,
            parent=parent,
            priority=int(spec.get("priority") or 0),
            max_attempts=int(spec.get("max_attempts") or default_attempts),
            task_id=child_id,
        )


class SpawnSubtaskTool(_SpawnBase):
    name = "spawn_subtask"
    description = (
        "Spawn ONE child task executed in parallel by another worker. Returns the "
        "child's task id immediately; results arrive later via wait_for_children. To "
        "launch MANY children at once, prefer spawn_batch (one call creates the whole "
        "burst). The child worker shares no context with you — its goal must be fully "
        "self-contained."
    )
    parameters: ClassVar[dict[str, Any]] = {
        "type": "object",
        "properties": _CHILD_SPEC_PROPERTIES,
        "required": ["goal"],
    }

    async def execute(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult:
        if ctx.tool_call_id is None:
            return ToolResult("spawn_subtask requires a tool call id", is_error=True)
        err = _spec_error(arguments)
        if err is not None:
            return ToolResult(err, is_error=True)
        resolved = self._resolve_node(arguments)
        if isinstance(resolved, ToolResult):
            return resolved
        agent, node = resolved
        sessions = _sessions_of(ctx)
        child_id = child_task_id(ctx.task_id, ctx.tool_call_id)

        async with session_scope(sessions) as session:
            if await session.get(Task, child_id) is not None:  # crash-recovery re-run
                return ToolResult(f"subtask already spawned: task {child_id}")
            parent = await session.get(Task, ctx.task_id)
            if parent is None:
                return ToolResult("parent task not found", is_error=True)
            budget_msg = await self._budget_exhausted(session, parent)
            if budget_msg is not None:
                return ToolResult(budget_msg, is_error=True)
            block = await self._swarm_health_block(session, parent)
            if block is not None:
                return ToolResult(block, is_error=True)
            if len(await _child_ids_of(session, parent.id)) >= self._max_subtasks:
                return ToolResult(
                    f"subtask cap reached ({self._max_subtasks}); integrate existing "
                    "children before spawning more",
                    is_error=True,
                )
            child_depth = int(parent.payload.get("depth") or 0) + 1
            if child_depth > MAX_SPAWN_DEPTH:
                return ToolResult(_depth_cap_message(), is_error=True)
            try:
                child = await self._enqueue_child(
                    session, arguments, parent, node, child_id, child_depth
                )
            except IntegrityError:  # lost a rare race with our own zombie
                return ToolResult(f"subtask already spawned: task {child_id}")
        label = f" as agent {agent!r}" if agent else ""
        return ToolResult(
            f"spawned subtask {child.id} ({child.kind.value}){label}: {child.payload['goal']}"
        )


class SpawnBatchTool(_SpawnBase):
    name = "spawn_batch"
    description = (
        "Spawn MANY child tasks AT ONCE — the whole parallel burst in a single call, "
        "all created in one transaction so they start simultaneously (far faster than "
        "spawn_subtask N times, which births children one slow turn apart). Pass a "
        "`children` array where each element is a self-contained child spec (same "
        "fields as spawn_subtask). Returns each child's task id; results arrive later "
        "via wait_for_children (call it ONCE after this). Each child shares no context "
        "with you — its goal must stand alone."
    )
    parameters: ClassVar[dict[str, Any]] = {
        "type": "object",
        "properties": {
            "children": {
                "type": "array",
                "description": "The child specs to launch in parallel, one per worker.",
                "items": {
                    "type": "object",
                    "properties": _CHILD_SPEC_PROPERTIES,
                    "required": ["goal"],
                },
            },
        },
        "required": ["children"],
    }

    async def execute(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult:
        if ctx.tool_call_id is None:
            return ToolResult("spawn_batch requires a tool call id", is_error=True)
        specs = arguments.get("children")
        if not isinstance(specs, list) or not specs:
            return ToolResult("spawn_batch needs a non-empty `children` array", is_error=True)
        # Validate + resolve every spec up front (fail fast, no partial batch).
        resolved: list[tuple[str | None, TeamNode | None]] = []
        for i, spec in enumerate(specs):
            if not isinstance(spec, dict):
                return ToolResult(f"children[{i}] must be an object", is_error=True)
            err = _spec_error(spec)
            if err is not None:
                return ToolResult(f"children[{i}]: {err}", is_error=True)
            outcome = self._resolve_node(spec)
            if isinstance(outcome, ToolResult):
                return ToolResult(f"children[{i}]: {outcome.content}", is_error=True)
            resolved.append(outcome)

        sessions = _sessions_of(ctx)
        batch_ids = [batch_child_id(ctx.task_id, ctx.tool_call_id, i) for i in range(len(specs))]
        # All inserts in ONE transaction (all-or-nothing). On a partial conflict
        # (concurrent zombie), the transaction rolls back; retry recomputes which
        # ids already exist and inserts only the missing set — converging.
        for _attempt in range(3):
            try:
                async with session_scope(sessions) as session:
                    parent = await session.get(Task, ctx.task_id)
                    if parent is None:
                        return ToolResult("parent task not found", is_error=True)
                    budget_msg = await self._budget_exhausted(session, parent)
                    if budget_msg is not None:
                        return ToolResult(budget_msg, is_error=True)
                    block = await self._swarm_health_block(session, parent)
                    if block is not None:
                        return ToolResult(block, is_error=True)
                    child_depth = int(parent.payload.get("depth") or 0) + 1
                    if child_depth > MAX_SPAWN_DEPTH:
                        return ToolResult(_depth_cap_message(), is_error=True)
                    present = await _child_ids_of(session, parent.id)
                    # Pure cap: count only children that are NOT this batch's own, so a
                    # replay (where some of our children already exist) accepts the
                    # identical prefix — accept element i iff base + i < cap.
                    base = len(present - set(batch_ids))
                    for i, spec in enumerate(specs):
                        if batch_ids[i] in present:
                            continue  # already spawned (replay / concurrent)
                        if base + i >= self._max_subtasks:
                            continue  # capped — deterministic by index
                        _agent, node = resolved[i]
                        await self._enqueue_child(
                            session, spec, parent, node, batch_ids[i], child_depth
                        )
                break
            except IntegrityError:
                continue  # a concurrent insert landed; recompute `present` and retry
        return ToolResult(await self._batch_report(sessions, specs, batch_ids))

    async def _batch_report(
        self, sessions: Sessions, specs: list[dict[str, Any]], batch_ids: list[uuid.UUID]
    ) -> str:
        async with session_scope(sessions) as session:
            rows = await session.scalars(sa.select(Task).where(Task.id.in_(batch_ids)))
            by_id = {c.id: c for c in rows}
        spawned = [
            {
                "task_id": str(cid),
                "goal": by_id[cid].payload.get("goal"),
                "kind": by_id[cid].kind.value,
            }
            for cid in batch_ids
            if cid in by_id
        ]
        skipped = [i for i, cid in enumerate(batch_ids) if cid not in by_id]
        report: dict[str, Any] = {"spawned": spawned, "count": len(spawned)}
        if skipped:
            report["skipped"] = {
                "indices": skipped,
                "reason": f"subtask cap ({self._max_subtasks}) reached — integrate existing "
                "children before spawning more",
            }
        return json.dumps(report, indent=2)


def _depth_cap_message() -> str:
    return (
        f"spawn depth cap reached (max {MAX_SPAWN_DEPTH}); this agent is too deep in the "
        "tree to spawn more children — do the work directly or report back to your parent"
    )


#: A child that reached one of these woke the leader early — no point blocking on
#: the survivors when something already failed.
_FAILURE_STATUSES = (TaskStatus.FAILED, TaskStatus.CANCELLED)


class WaitForChildrenTool(Tool):
    name = "wait_for_children"
    description = (
        "Sleep (at zero compute cost) until every spawned subtask has finished, "
        "then receive a report of each child's status and result. Spawn ALL the "
        "children you need first (they run concurrently), then call this ONCE to "
        "wait for the whole batch — do not spawn one, wait, spawn the next. It WAKES "
        "YOU EARLY if a child fails or is cancelled while the others are still "
        "running, so you can react (respawn a fix, adjust the plan, integrate what "
        "works, or stop the rest) instead of blocking on the survivors; call it again "
        "to keep waiting — it only re-reports NEW failures. To act on children as they "
        "finish individually instead, poll agent_status. Never poll in a busy loop."
    )
    parameters: ClassVar[dict[str, Any]] = {"type": "object", "properties": {}}
    #: Re-running after a crash just re-checks state — naturally idempotent.
    idempotency = ToolIdempotency.IDEMPOTENT

    async def execute(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult:
        sessions = _sessions_of(ctx)
        async with session_scope(sessions) as session:
            children = list(
                (
                    await session.scalars(
                        sa.select(Task)
                        .where(Task.parent_task_id == ctx.task_id)
                        .order_by(Task.created_at, Task.id)
                    )
                ).all()
            )
            if not children:
                return ToolResult("no subtasks have been spawned; nothing to wait for")
            if all(c.status in TERMINAL_STATUSES for c in children):
                return ToolResult(_children_report(children))
            # Children still run. Return NOW on a not-yet-surfaced failure so the
            # leader can react; record which failures we surfaced (durably) so a
            # later wait blocks for the survivors instead of re-reporting the same
            # ones — that is what lets a leader respawn a fix and wait again.
            failed = [c for c in children if c.status in _FAILURE_STATUSES]
            surfaced = await _surfaced_failed_ids(session, ctx.task_id) if failed else set()
            new_failed = [c for c in failed if str(c.id) not in surfaced]
            if new_failed:
                await append_event(
                    session,
                    ctx.task_id,
                    EventType.CHILDREN_FAILED_EARLY,
                    {"failed": sorted(str(c.id) for c in failed)},
                )
                return ToolResult(_early_failure_report(children, new_failed))
        # Nothing new to report — sleep until the next child settles (or all do).
        raise TaskParked(TaskStatus.WAITING_CHILDREN.value)


async def _surfaced_failed_ids(session: AsyncSession, task_id: uuid.UUID) -> set[str]:
    """Child ids a prior wait_for_children already surfaced as failed — read from
    the durable CHILDREN_FAILED_EARLY events, so a resume decides identically and a
    leader is not re-woken for the same failure it already saw."""
    surfaced: set[str] = set()
    for event in await read_events(session, task_id):
        if event.event_type is EventType.CHILDREN_FAILED_EARLY:
            surfaced.update(str(fid) for fid in event.payload.get("failed", []))
    return surfaced


def _early_failure_report(children: list[Task], new_failed: list[Task]) -> str:
    """A compact report for an early (child-failure) wake: the newly-failed children
    with their errors, plus what is still running, so the leader can decide fast."""
    running = [c for c in children if c.status not in TERMINAL_STATUSES]
    succeeded = sum(1 for c in children if c.status is TaskStatus.SUCCEEDED)
    failed = sum(1 for c in children if c.status in _FAILURE_STATUSES)
    body = {
        "early_exit": "child_failure",
        "note": (
            "Woke early: a child FAILED or was CANCELLED while others are still "
            "running. Decide now — respawn a focused fix, adjust the plan, integrate "
            "and land what already works, or stop the rest. To keep waiting for the "
            "still-running children, call wait_for_children again; it only re-reports "
            "NEW failures."
        ),
        "summary": {"failed": failed, "running": len(running), "succeeded": succeeded},
        "newly_failed": [
            {
                "task_id": str(c.id),
                "goal": c.payload.get("goal"),
                "status": c.status.value,
                **({"error": c.last_error[:_ERROR_CHARS]} if c.last_error else {}),
            }
            for c in new_failed
        ],
        "still_running": [{"task_id": str(c.id), "goal": c.payload.get("goal")} for c in running],
    }
    return json.dumps(body, indent=2)


def _children_report(children: list[Task]) -> str:
    report: list[dict[str, Any]] = []
    for child in children:
        result = child.result or {}
        entry: dict[str, Any] = {
            "task_id": str(child.id),
            "goal": child.payload.get("goal"),
            "status": child.status.value,
        }
        if child.status is TaskStatus.SUCCEEDED:
            if result.get("branch"):
                entry["branch"] = result["branch"]
            tail = str(result.get("final_text") or "").strip()
            if tail:
                # Just the closing tail — enough to see how the worker finished; the
                # integratable artifact is the branch, not the prose.
                entry["result_tail"] = tail[-_SUCCESS_TAIL_CHARS:]
        elif child.last_error:
            entry["error"] = child.last_error[:_ERROR_CHARS]
        report.append(entry)
    counts = {
        status.value: n
        for status in TERMINAL_STATUSES
        if (n := sum(1 for c in children if c.status is status))
    }

    def _render(entries: list[dict[str, Any]], note: str | None = None) -> str:
        body: dict[str, Any] = {"summary": counts, "children": entries}
        if note is not None:
            body["note"] = note
        return json.dumps(body, indent=2)

    text = _render(report)
    if len(text) <= _REPORT_CHAR_CAP:
        return text
    # A very large wave still over the ceiling: drop success tails (keeping every
    # branch + full error). If STILL too big, keep all failures and as many succeeded
    # entries as fit, and say how many were omitted — the counts remain exact and
    # merge_child_branches auto-discovers every child branch from the DB, so trimming
    # succeeded entries here loses nothing the leader needs. Always valid JSON.
    for entry in report:
        entry.pop("result_tail", None)
    failures = [e for e in report if "error" in e]
    successes = [e for e in report if "error" not in e]
    kept: list[dict[str, Any]] = []
    for entry in successes:
        if len(_render(failures + kept + [entry])) > _REPORT_CHAR_CAP:
            break
        kept.append(entry)
    omitted = len(successes) - len(kept)
    note = "success tails dropped to fit; statuses, branches, and errors kept."
    if omitted:
        note = (
            f"{omitted} succeeded children omitted to fit (summary counts are exact); "
            "merge_child_branches finds every child branch automatically."
        )
    return _render(failures + kept, note)


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
    max_subtasks: int = DEFAULT_MAX_SUBTASKS,
    team: TeamNode | None = None,
    *,
    max_repair_failures: int = DEFAULT_MAX_REPAIR_FAILURES,
    run_task_ceiling: int = DEFAULT_RUN_TASK_CEILING,
) -> list[Tool]:
    """The delegation toolset: spawn (one or a batch)/wait plus per-child
    status/terminate. The two spawn tools share the same circuit-breaker limits."""
    limits = {"max_repair_failures": max_repair_failures, "run_task_ceiling": run_task_ceiling}
    return [
        SpawnSubtaskTool(max_subtasks, team=team, **limits),
        SpawnBatchTool(max_subtasks, team=team, **limits),
        WaitForChildrenTool(),
        AgentStatusTool(),
        AgentTerminateTool(),
    ]


def build_planner_registry(
    max_subtasks: int = DEFAULT_MAX_SUBTASKS,
    team: TeamNode | None = None,
    *,
    max_repair_failures: int = DEFAULT_MAX_REPAIR_FAILURES,
    run_task_ceiling: int = DEFAULT_RUN_TASK_CEILING,
) -> ToolRegistry:
    from gantry.worker.tools.ask import AskUserTool

    registry = ToolRegistry(
        orchestration_tools(
            max_subtasks,
            team=team,
            max_repair_failures=max_repair_failures,
            run_task_ceiling=run_task_ceiling,
        )
    )
    registry.register(AskUserTool())
    return registry
