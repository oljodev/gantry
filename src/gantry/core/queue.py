"""Durable task queue operations.

Concurrency model:

- **Claiming** is a single statement: a CTE selects the next runnable task
  with ``FOR UPDATE SKIP LOCKED`` and the outer UPDATE claims it. Thousands
  of workers can poll concurrently without collisions or lock waits.
- **Leases, not held locks.** A claim grants a lease (``lease_expires_at``);
  workers heartbeat to extend it. The reaper re-queues tasks whose lease
  expired (worker died) — combined with event-log replay, a killed worker
  loses only its in-flight step.
- **Fencing.** ``attempt`` increments atomically at claim time and every
  worker mutation must present (worker_id, attempt). A zombie worker whose
  task was reaped and re-claimed can no longer complete/fail/heartbeat it.
- All timestamps come from the database clock (``now()``), never the host's.
"""

from __future__ import annotations

import uuid
from dataclasses import dataclass
from datetime import datetime
from typing import Any, cast

import sqlalchemy as sa
from sqlalchemy.ext.asyncio import AsyncSession

from gantry.core.events import append_event
from gantry.core.models import (
    DEFAULT_PROJECT_ID,
    LEASED_STATUSES,
    TERMINAL_STATUSES,
    EventType,
    Task,
    TaskEvent,
    TaskKind,
    TaskStatus,
)
from gantry.core.notify import notify_task_cancel, notify_task_ready
from gantry.logging import get_logger

logger = get_logger(__name__)

DEFAULT_LEASE_SECONDS = 60.0
DEFAULT_RETRY_BACKOFF_BASE_SECONDS = 5.0
DEFAULT_RETRY_BACKOFF_CAP_SECONDS = 300.0


def _rowcount(result: sa.Result[Any]) -> int:
    return cast("sa.CursorResult[Any]", result).rowcount


def _now_plus(seconds: float) -> sa.ColumnElement[datetime]:
    return sa.func.now() + sa.func.make_interval(0, 0, 0, 0, 0, 0, seconds)


#: Extra max_attempts headroom granted on a park/wake resume. Waking a parked
#: task re-queues it PENDING; the next claim bumps `attempt` (the fencing token).
#: Because `attempt` doubles as the error-retry counter (will_retry = attempt <
#: max_attempts), every park/wake would otherwise burn a retry — so a many-wave
#: leader hits max_attempts and terminally FAILs, orphaning its whole subtree.
#: Lifting the ceiling on wake decouples the two: `attempt` still increments
#: (fencing intact), but the wake never consumes the error budget.
_WAKE_ATTEMPT_HEADROOM = 3


async def run_spend_usd(session: AsyncSession, root_task_id: uuid.UUID) -> float:
    """Total USD spent by every task in a run (its whole tree), read from the
    committed per-task ``cost_usd`` — the signal the per-run budget brake consults
    before a leader fans out another wave. In-flight tasks contribute 0 until they
    reach a terminal transition, so this is the settled spend (conservative)."""
    total = await session.scalar(
        sa.select(sa.func.coalesce(sa.func.sum(Task.cost_usd), 0.0)).where(
            Task.root_task_id == root_task_id
        )
    )
    return float(total or 0.0)


async def failed_child_count(session: AsyncSession, parent_id: uuid.UUID) -> int:
    """How many of a task's DIRECT children have terminally FAILED — the repair-wave
    breaker's signal. Committed rows only, so a crash-replay reads the same count.
    Cancellations don't count: a deliberately stopped child is not a failing approach.
    """
    n = await session.scalar(
        sa.select(sa.func.count()).where(
            Task.parent_task_id == parent_id, Task.status == TaskStatus.FAILED
        )
    )
    return int(n or 0)


async def run_task_count(session: AsyncSession, root_task_id: uuid.UUID) -> int:
    """Total tasks in a run's whole tree — the structural fan-out backstop the spawn
    tools consult so a deeply nested swarm can't exceed the run-wide task ceiling."""
    n = await session.scalar(sa.select(sa.func.count()).where(Task.root_task_id == root_task_id))
    return int(n or 0)


async def run_rollup(session: AsyncSession, root_task_id: uuid.UUID) -> dict[str, Any]:
    """A run-level observability rollup over a whole tree: per-status task counts,
    settled spend, folded token sums, prompt-cache hit ratio, and total compactions.
    Read-only and derived from committed rows/result JSON — never a replay input."""

    def _jsum(field: str) -> Any:
        # SUM ignores NULLs, so tasks without a result (or that key) contribute 0.
        return sa.func.coalesce(sa.func.sum(sa.cast(Task.result[field].astext, sa.Float)), 0.0)

    agg = (
        await session.execute(
            sa.select(
                sa.func.count().label("tasks"),
                sa.func.coalesce(sa.func.sum(Task.cost_usd), 0.0).label("spent_usd"),
                _jsum("prompt_tokens").label("prompt_tokens"),
                _jsum("completion_tokens").label("completion_tokens"),
                _jsum("cache_read_tokens").label("cache_read_tokens"),
                _jsum("compactions").label("compactions"),
            ).where(Task.root_task_id == root_task_id)
        )
    ).one()
    status_rows = (
        await session.execute(
            sa.select(Task.status, sa.func.count())
            .where(Task.root_task_id == root_task_id)
            .group_by(Task.status)
        )
    ).all()
    prompt = float(agg.prompt_tokens)
    cache_read = float(agg.cache_read_tokens)
    return {
        "tasks": int(agg.tasks),
        "statuses": {getattr(s, "value", str(s)): int(n) for s, n in status_rows},
        "spent_usd": round(float(agg.spent_usd), 4),
        "prompt_tokens": int(prompt),
        "completion_tokens": int(agg.completion_tokens),
        "cache_read_tokens": int(cache_read),
        "cache_hit_ratio": round(cache_read / prompt, 3) if prompt else 0.0,
        "compactions": int(agg.compactions),
    }


def _wake_to_pending() -> dict[str, Any]:
    """The UPDATE values that re-queue a parked task, lifting max_attempts so the
    wake doesn't erode the error-retry budget. Monotonic ``greatest()`` is
    idempotent under concurrent sibling wakes (the row is locked first)."""
    return {
        "status": TaskStatus.PENDING,
        "scheduled_at": sa.func.now(),
        "updated_at": sa.func.now(),
        "max_attempts": sa.func.greatest(Task.max_attempts, Task.attempt + _WAKE_ATTEMPT_HEADROOM),
    }


def retry_backoff_seconds(
    attempt: int,
    base: float = DEFAULT_RETRY_BACKOFF_BASE_SECONDS,
    cap: float = DEFAULT_RETRY_BACKOFF_CAP_SECONDS,
) -> float:
    """Exponential backoff for retry N (attempt is 1-based): base * 2^(n-1), capped."""
    return min(base * float(2 ** max(attempt - 1, 0)), cap)


@dataclass(frozen=True)
class ReapedTask:
    task_id: uuid.UUID
    status: TaskStatus
    attempt: int
    claimed_by: str | None


@dataclass(frozen=True)
class Heartbeat:
    """Outcome of a lease heartbeat.

    Truthy iff the lease is still held, so existing ``if await heartbeat(...)``
    call sites keep working; ``cancel_requested`` rides along so the worker
    learns of an operator cancel in the same round-trip.
    """

    alive: bool
    cancel_requested: bool = False

    def __bool__(self) -> bool:
        return self.alive


async def enqueue(
    session: AsyncSession,
    *,
    workspace_id: uuid.UUID,
    kind: TaskKind,
    payload: dict[str, Any],
    parent: Task | None = None,
    project_id: uuid.UUID | None = None,
    priority: int = 0,
    max_attempts: int = 3,
    scheduled_at: datetime | None = None,
    task_id: uuid.UUID | None = None,
) -> Task:
    """Insert a pending task, log it, and notify idle workers (on commit).

    ``task_id`` may be supplied for deterministic (exactly-once) enqueueing —
    e.g. ``spawn_subtask`` derives it from the spawning tool call's identity.
    A child inherits its parent's ``project_id``; a root uses the supplied
    ``project_id`` (falling back to the Default project).
    """
    task_id = task_id or uuid.uuid4()
    task = Task(
        id=task_id,
        workspace_id=workspace_id,
        project_id=project_id or (parent.project_id if parent else DEFAULT_PROJECT_ID),
        parent_task_id=parent.id if parent else None,
        root_task_id=parent.root_task_id if parent else task_id,
        kind=kind,
        payload=payload,
        priority=priority,
        max_attempts=max_attempts,
    )
    if scheduled_at is not None:
        task.scheduled_at = scheduled_at
    session.add(task)
    await session.flush()
    await append_event(
        session,
        task.id,
        EventType.TASK_ENQUEUED,
        {"kind": kind.value, "priority": priority, "parent_task_id": _opt(task.parent_task_id)},
    )
    await notify_task_ready(session, task.id)
    return task


async def claim(
    session: AsyncSession,
    *,
    worker_id: str,
    lease_seconds: float = DEFAULT_LEASE_SECONDS,
) -> Task | None:
    """Atomically claim the next runnable task, or return None if queue is empty.

    Highest priority first, then earliest scheduled_at (so retried tasks
    re-enter fairly). SKIP LOCKED means concurrent claimers never wait on or
    receive the same row.
    """
    next_task = (
        sa.select(Task.id)
        .where(Task.status == TaskStatus.PENDING, Task.scheduled_at <= sa.func.now())
        .order_by(Task.priority.desc(), Task.scheduled_at)
        .limit(1)
        .with_for_update(skip_locked=True)
        .cte("next_task")
    )
    stmt = (
        sa.update(Task)
        .where(Task.id == sa.select(next_task.c.id).scalar_subquery())
        .values(
            status=TaskStatus.CLAIMED,
            claimed_by=worker_id,
            attempt=Task.attempt + 1,
            lease_expires_at=_now_plus(lease_seconds),
            updated_at=sa.func.now(),
        )
        .returning(Task)
    )
    task = (await session.scalars(stmt)).first()
    if task is None:
        return None
    await append_event(
        session,
        task.id,
        EventType.TASK_CLAIMED,
        {"worker_id": worker_id, "attempt": task.attempt},
    )
    return task


async def heartbeat(
    session: AsyncSession,
    *,
    task_id: uuid.UUID,
    worker_id: str,
    attempt: int,
    lease_seconds: float = DEFAULT_LEASE_SECONDS,
) -> Heartbeat:
    """Extend the lease and report whether a cancel was requested.

    A falsy result (lease lost) means the worker must abort; a truthy result
    with ``cancel_requested`` set means an operator asked to stop this task.
    """
    row = (
        await session.execute(
            sa.update(Task)
            .where(
                Task.id == task_id,
                Task.claimed_by == worker_id,
                Task.attempt == attempt,
                Task.status.in_(LEASED_STATUSES),
            )
            .values(lease_expires_at=_now_plus(lease_seconds), updated_at=sa.func.now())
            .returning(Task.cancel_requested)
        )
    ).first()
    if row is None:
        return Heartbeat(alive=False)
    return Heartbeat(alive=True, cancel_requested=bool(row[0]))


async def mark_running(
    session: AsyncSession,
    *,
    task_id: uuid.UUID,
    worker_id: str,
    attempt: int,
) -> bool:
    result = await session.execute(
        sa.update(Task)
        .where(
            Task.id == task_id,
            Task.claimed_by == worker_id,
            Task.attempt == attempt,
            Task.status == TaskStatus.CLAIMED,
        )
        .values(status=TaskStatus.RUNNING, updated_at=sa.func.now())
    )
    return _rowcount(result) == 1


async def complete(
    session: AsyncSession,
    *,
    task_id: uuid.UUID,
    worker_id: str,
    attempt: int,
    result: dict[str, Any] | None = None,
    cost_usd: float = 0.0,
) -> bool:
    """Mark succeeded. Transactional with its event — completion is exactly-once.

    ``cost_usd`` is SET (not incremented) inside this fenced terminal UPDATE, so a
    re-claim/zombie can never double-count the run's spend."""
    res = await session.execute(
        sa.update(Task)
        .where(
            Task.id == task_id,
            Task.claimed_by == worker_id,
            Task.attempt == attempt,
            Task.status.in_(LEASED_STATUSES),
        )
        .values(
            status=TaskStatus.SUCCEEDED,
            result=result,
            cost_usd=cost_usd,
            claimed_by=None,
            lease_expires_at=None,
            updated_at=sa.func.now(),
        )
    )
    if _rowcount(res) != 1:
        return False
    await append_event(
        session,
        task_id,
        EventType.TASK_SUCCEEDED,
        {"worker_id": worker_id, "attempt": attempt},
    )
    await _try_wake_parent(session, await _parent_of(session, task_id))
    return True


async def fail(
    session: AsyncSession,
    *,
    task_id: uuid.UUID,
    worker_id: str,
    attempt: int,
    error: str,
    retryable: bool = True,
    backoff_base_seconds: float = DEFAULT_RETRY_BACKOFF_BASE_SECONDS,
    backoff_cap_seconds: float = DEFAULT_RETRY_BACKOFF_CAP_SECONDS,
) -> TaskStatus | None:
    """Record a failure: re-queue with backoff, or fail terminally.

    Returns the resulting status, or None if the fencing check rejected the
    write (lease lost — some other attempt owns the task now).
    """
    guard = (
        Task.id == task_id,
        Task.claimed_by == worker_id,
        Task.attempt == attempt,
        Task.status.in_(LEASED_STATUSES),
    )
    will_retry = retryable and attempt < await _max_attempts_of(session, task_id)
    if will_retry:
        delay = retry_backoff_seconds(attempt, backoff_base_seconds, backoff_cap_seconds)
        res = await session.execute(
            sa.update(Task)
            .where(*guard)
            .values(
                status=TaskStatus.PENDING,
                last_error=error,
                claimed_by=None,
                lease_expires_at=None,
                scheduled_at=_now_plus(delay),
                updated_at=sa.func.now(),
            )
        )
        if _rowcount(res) != 1:
            return None
        await append_event(
            session,
            task_id,
            EventType.TASK_RETRY_SCHEDULED,
            {"worker_id": worker_id, "attempt": attempt, "error": error, "delay_seconds": delay},
        )
        return TaskStatus.PENDING

    res = await session.execute(
        sa.update(Task)
        .where(*guard)
        .values(
            status=TaskStatus.FAILED,
            last_error=error,
            claimed_by=None,
            lease_expires_at=None,
            updated_at=sa.func.now(),
        )
    )
    if _rowcount(res) != 1:
        return None
    await append_event(
        session,
        task_id,
        EventType.TASK_FAILED,
        {"worker_id": worker_id, "attempt": attempt, "error": error},
    )
    await _try_wake_parent(session, await _parent_of(session, task_id))
    return TaskStatus.FAILED


async def escalate(
    session: AsyncSession,
    *,
    task_id: uuid.UUID,
    worker_id: str,
    attempt: int,
    fingerprint: str,
    history: list[str],
    model: str | None,
) -> TaskStatus | None:
    """Record a stalled task's escalation and re-queue it. Returns the new status.

    The escalation is an EVENT, not a payload edit: the payload is the immutable
    launch snapshot, and rewriting it would make a task's history disagree with
    what it was launched as. Rehydration folds the event into both the model
    choice and the opening context, so the escalation survives a crash and a
    re-claim exactly like every other decision this engine makes.

    ``max_attempts`` gets headroom for the same reason a park/wake does: the
    re-claim increments ``attempt`` (the fencing token), and without headroom a
    stalled task would spend an error retry just to be handed to a better model.
    """
    res = await session.execute(
        sa.update(Task)
        .where(
            Task.id == task_id,
            Task.claimed_by == worker_id,
            Task.attempt == attempt,
            Task.status.in_(LEASED_STATUSES),
        )
        .values(**_wake_to_pending(), claimed_by=None, lease_expires_at=None)
    )
    if _rowcount(res) != 1:
        return None
    await append_event(
        session,
        task_id,
        EventType.TASK_ESCALATED,
        {"fingerprint": fingerprint, "history": history, "model": model},
    )
    await notify_task_ready(session, task_id)
    logger.warning(
        "queue.task_escalated", task_id=str(task_id), fingerprint=fingerprint, model=model
    )
    return TaskStatus.PENDING


async def park_for_children(
    session: AsyncSession,
    *,
    task_id: uuid.UUID,
    worker_id: str,
    attempt: int,
) -> TaskStatus | None:
    """Park a planner until its children settle. Returns the resulting status.

    Fenced like every worker mutation. The park and the lost-wakeup guard run
    in ONE transaction: after switching to ``waiting_children`` we re-check
    the children — if they all reached terminal states while we were deciding
    to park (their completions saw a non-parked parent and skipped the wake),
    we immediately flip back to ``pending``. A child completing concurrently
    blocks on this row's lock and re-evaluates after our commit, so exactly
    one side always delivers the wakeup.
    """
    res = await session.execute(
        sa.update(Task)
        .where(
            Task.id == task_id,
            Task.claimed_by == worker_id,
            Task.attempt == attempt,
            Task.status.in_(LEASED_STATUSES),
        )
        .values(
            status=TaskStatus.WAITING_CHILDREN,
            claimed_by=None,
            lease_expires_at=None,
            updated_at=sa.func.now(),
        )
    )
    if _rowcount(res) != 1:
        return None
    await append_event(session, task_id, EventType.TASK_PARKED, {"reason": "waiting_children"})
    if await _try_wake_parent(session, task_id):
        return TaskStatus.PENDING
    return TaskStatus.WAITING_CHILDREN


async def _try_wake_parent(session: AsyncSession, parent_id: uuid.UUID | None) -> bool:
    """Re-queue a parked parent iff every child has reached a terminal state.

    Ordering is load-bearing: LOCK THE PARENT ROW FIRST, then examine the
    children. Two siblings completing concurrently under READ COMMITTED would
    otherwise each see the other's uncommitted row as unfinished and both skip
    the wake (write skew) — parking the parent forever. With the lock taken
    first, the second completer blocks until the first commits and then
    re-reads the children with that commit visible, so the last one to finish
    always delivers the wakeup. (The park path takes the same lock via its own
    UPDATE, which is what closes the park-vs-complete race too.)
    """
    if parent_id is None:
        return False
    status = await session.scalar(
        sa.select(Task.status).where(Task.id == parent_id).with_for_update()
    )
    if status is None or TaskStatus(status) is not TaskStatus.WAITING_CHILDREN:
        return False
    unfinished = await session.scalar(
        sa.select(
            sa.select(Task.id)
            .where(Task.parent_task_id == parent_id, Task.status.notin_(TERMINAL_STATUSES))
            .exists()
        )
    )
    if unfinished:
        return False
    await session.execute(sa.update(Task).where(Task.id == parent_id).values(**_wake_to_pending()))
    await append_event(session, parent_id, EventType.TASK_RESUMED, {"reason": "children_settled"})
    await notify_task_ready(session, parent_id)
    logger.info("queue.parent_woken", parent_id=str(parent_id))
    return True


async def park_for_approval(
    session: AsyncSession,
    *,
    task_id: uuid.UUID,
    worker_id: str,
    attempt: int,
) -> TaskStatus | None:
    """Park a task until a human resolves its pending approval request.

    Same shape as :func:`park_for_children`: the fenced park UPDATE locks the
    task row, and the lost-wakeup guard runs in the same transaction — if the
    operator resolved the request in the window between the gate emitting
    ``approval_requested`` and this park committing, we see their committed
    event here (they lock the row too, so we serialize) and immediately
    re-queue instead of parking forever.
    """
    res = await session.execute(
        sa.update(Task)
        .where(
            Task.id == task_id,
            Task.claimed_by == worker_id,
            Task.attempt == attempt,
            Task.status.in_(LEASED_STATUSES),
        )
        .values(
            status=TaskStatus.WAITING_APPROVAL,
            claimed_by=None,
            lease_expires_at=None,
            updated_at=sa.func.now(),
        )
    )
    if _rowcount(res) != 1:
        return None
    await append_event(session, task_id, EventType.TASK_PARKED, {"reason": "waiting_approval"})
    if not await _unresolved_approval_ids(session, task_id):
        await session.execute(
            sa.update(Task).where(Task.id == task_id).values(**_wake_to_pending())
        )
        await append_event(
            session, task_id, EventType.TASK_RESUMED, {"reason": "approval_resolved"}
        )
        await notify_task_ready(session, task_id)
        return TaskStatus.PENDING
    return TaskStatus.WAITING_APPROVAL


async def resolve_approval(
    session: AsyncSession,
    *,
    task_id: uuid.UUID,
    tool_call_id: str,
    approved: bool,
    comment: str = "",
    resolved_by: str = "operator",
) -> tuple[str, Task | None]:
    """Record a human decision for a gated tool call and wake the task.

    Returns ("resolved" | "not_found" | "already_resolved", task). Locks the
    task row FIRST so this serializes with a concurrent park (see
    :func:`park_for_approval`); whichever commits second delivers the wakeup.
    """
    task = await session.get(Task, task_id, with_for_update=True)
    if task is None:
        return "not_found", None
    requested, resolved = await _approval_ledger(session, task_id)
    if tool_call_id not in requested:
        return "not_found", task
    if tool_call_id in resolved:
        return "already_resolved", task

    await append_event(
        session,
        task_id,
        EventType.APPROVAL_RESOLVED,
        {
            "tool_call_id": tool_call_id,
            "decision": "approved" if approved else "rejected",
            "comment": comment,
            "resolved_by": resolved_by,
        },
    )
    if task.status is TaskStatus.WAITING_APPROVAL and not await _unresolved_approval_ids(
        session, task_id
    ):
        await session.execute(
            sa.update(Task).where(Task.id == task_id).values(**_wake_to_pending())
        )
        await append_event(
            session, task_id, EventType.TASK_RESUMED, {"reason": "approval_resolved"}
        )
        await notify_task_ready(session, task_id)
        await session.refresh(task)
    logger.info(
        "queue.approval_resolved",
        task_id=str(task_id),
        tool_call_id=tool_call_id,
        approved=approved,
    )
    return "resolved", task


async def _approval_ledger(session: AsyncSession, task_id: uuid.UUID) -> tuple[set[str], set[str]]:
    """(requested, resolved) tool_call_ids from the task's approval events."""
    events = await session.scalars(
        sa.select(TaskEvent).where(
            TaskEvent.task_id == task_id,
            TaskEvent.event_type.in_(
                [EventType.APPROVAL_REQUESTED.value, EventType.APPROVAL_RESOLVED.value]
            ),
        )
    )
    requested, resolved = set(), set()
    for event in events:
        call_id = str(event.payload.get("tool_call_id"))
        if EventType(event.event_type) is EventType.APPROVAL_REQUESTED:
            requested.add(call_id)
        else:
            resolved.add(call_id)
    return requested, resolved


async def _unresolved_approval_ids(session: AsyncSession, task_id: uuid.UUID) -> set[str]:
    requested, resolved = await _approval_ledger(session, task_id)
    return requested - resolved


async def park_for_input(
    session: AsyncSession,
    *,
    task_id: uuid.UUID,
    worker_id: str,
    attempt: int,
) -> TaskStatus | None:
    """Park a task until a human answers its pending ask_user question.

    Identical machinery to :func:`park_for_approval` (same fenced park +
    lost-wakeup guard in one transaction), keyed on the ask_user question
    ledger instead of the approval ledger.
    """
    res = await session.execute(
        sa.update(Task)
        .where(
            Task.id == task_id,
            Task.claimed_by == worker_id,
            Task.attempt == attempt,
            Task.status.in_(LEASED_STATUSES),
        )
        .values(
            status=TaskStatus.WAITING_INPUT,
            claimed_by=None,
            lease_expires_at=None,
            updated_at=sa.func.now(),
        )
    )
    if _rowcount(res) != 1:
        return None
    await append_event(session, task_id, EventType.TASK_PARKED, {"reason": "waiting_input"})
    if not await _unresolved_question_ids(session, task_id):
        await session.execute(
            sa.update(Task).where(Task.id == task_id).values(**_wake_to_pending())
        )
        await append_event(session, task_id, EventType.TASK_RESUMED, {"reason": "input_answered"})
        await notify_task_ready(session, task_id)
        return TaskStatus.PENDING
    return TaskStatus.WAITING_INPUT


async def resolve_input(
    session: AsyncSession,
    *,
    task_id: uuid.UUID,
    tool_call_id: str,
    answer: str,
    resolved_by: str = "operator",
) -> tuple[str, Task | None]:
    """Record a human's answer to an ask_user question and wake the task.

    Returns ("resolved" | "not_found" | "already_resolved", task). Locks the
    task row FIRST so this serializes with a concurrent park (see
    :func:`resolve_approval`); whichever commits second delivers the wakeup.
    """
    task = await session.get(Task, task_id, with_for_update=True)
    if task is None:
        return "not_found", None
    asked, answered = await _question_ledger(session, task_id)
    if tool_call_id not in asked:
        return "not_found", task
    if tool_call_id in answered:
        return "already_resolved", task

    await append_event(
        session,
        task_id,
        EventType.ASK_USER_ANSWERED,
        {"tool_call_id": tool_call_id, "answer": answer, "resolved_by": resolved_by},
    )
    if task.status is TaskStatus.WAITING_INPUT and not await _unresolved_question_ids(
        session, task_id
    ):
        await session.execute(
            sa.update(Task).where(Task.id == task_id).values(**_wake_to_pending())
        )
        await append_event(session, task_id, EventType.TASK_RESUMED, {"reason": "input_answered"})
        await notify_task_ready(session, task_id)
        await session.refresh(task)
    logger.info("queue.input_answered", task_id=str(task_id), tool_call_id=tool_call_id)
    return "resolved", task


async def _question_ledger(session: AsyncSession, task_id: uuid.UUID) -> tuple[set[str], set[str]]:
    """(asked, answered) tool_call_ids from the task's ask_user events."""
    events = await session.scalars(
        sa.select(TaskEvent).where(
            TaskEvent.task_id == task_id,
            TaskEvent.event_type.in_(
                [EventType.ASK_USER_QUESTION.value, EventType.ASK_USER_ANSWERED.value]
            ),
        )
    )
    asked, answered = set(), set()
    for event in events:
        call_id = str(event.payload.get("tool_call_id"))
        if EventType(event.event_type) is EventType.ASK_USER_QUESTION:
            asked.add(call_id)
        else:
            answered.add(call_id)
    return asked, answered


async def _unresolved_question_ids(session: AsyncSession, task_id: uuid.UUID) -> set[str]:
    asked, answered = await _question_ledger(session, task_id)
    return asked - answered


async def retry(session: AsyncSession, *, task_id: uuid.UUID) -> Task | None:
    """Manually re-queue a terminally failed or cancelled task.

    The event log survives, so the retried task resumes from its last
    checkpoint rather than starting over. ``max_attempts`` is raised to give
    the retry real headroom (a terminal failure means attempts were already
    exhausted). Returns None if the task isn't in a retryable state.
    """
    stmt = (
        sa.update(Task)
        .where(Task.id == task_id, Task.status.in_([TaskStatus.FAILED, TaskStatus.CANCELLED]))
        .values(
            status=TaskStatus.PENDING,
            scheduled_at=sa.func.now(),
            max_attempts=sa.func.greatest(Task.max_attempts, Task.attempt + 3),
            updated_at=sa.func.now(),
        )
        .returning(Task)
    )
    task = (await session.scalars(stmt)).first()
    if task is None:
        return None
    await append_event(
        session,
        task.id,
        EventType.TASK_RETRY_SCHEDULED,
        {"manual": True, "attempt": task.attempt, "delay_seconds": 0},
    )
    await notify_task_ready(session, task.id)
    logger.info("queue.task_retried", task_id=str(task_id))
    return task


#: Statuses that are parked (no worker owns the loop) yet not terminal — safe
#: to cancel outright, exactly like PENDING.
_CANCELLABLE_NOW = (
    TaskStatus.PENDING,
    TaskStatus.WAITING_APPROVAL,
    TaskStatus.WAITING_CHILDREN,
)


@dataclass(frozen=True)
class CancelResult:
    """What ``cancel`` did: ``task`` is the row, ``requested`` is True when a
    live worker must still cooperatively stop (task not yet terminal)."""

    task: Task
    requested: bool


async def cancel(session: AsyncSession, *, task_id: uuid.UUID) -> CancelResult | None:
    """Cancel a task in any non-terminal state.

    - PENDING / parked (waiting_approval, waiting_children): no worker owns the
      loop, so flip straight to CANCELLED (terminal).
    - CLAIMED / RUNNING: a lease-holding worker owns it, so we can't yank the
      row terminal from under it (its next write would conflict). Instead set
      ``cancel_requested``; the worker sees it at its next heartbeat and
      transitions to CANCELLED itself via :func:`mark_cancelled`.

    Returns None if the task doesn't exist or is already terminal.
    """
    stmt = (
        sa.update(Task)
        .where(Task.id == task_id, Task.status.in_(_CANCELLABLE_NOW))
        .values(
            status=TaskStatus.CANCELLED,
            claimed_by=None,
            lease_expires_at=None,
            updated_at=sa.func.now(),
        )
        .returning(Task)
    )
    task = (await session.scalars(stmt)).first()
    if task is not None:
        await append_event(session, task.id, EventType.TASK_CANCELLED, {})
        await _try_wake_parent(session, task.parent_task_id)  # cancellation is terminal too
        return CancelResult(task=task, requested=False)

    # Live (leased) task: request cooperative cancellation.
    requested = (
        (
            await session.execute(
                sa.update(Task)
                .where(Task.id == task_id, Task.status.in_(LEASED_STATUSES))
                .values(cancel_requested=True, updated_at=sa.func.now())
                .returning(Task)
            )
        )
        .scalars()
        .first()
    )
    if requested is None:
        return None  # gone or already terminal
    # Wake the owning worker now so it interrupts the in-flight step, rather than
    # waiting for its next heartbeat poll (the cooperative path is the fallback).
    await notify_task_cancel(session, task_id)
    return CancelResult(task=requested, requested=True)


async def mark_cancelled(
    session: AsyncSession,
    *,
    task_id: uuid.UUID,
    worker_id: str,
    attempt: int,
) -> bool:
    """Worker-side terminal transition after an honoured cancel request.

    Compare-and-set on the lease so a zombie whose lease was reaped can't
    cancel a task some other attempt now owns.
    """
    task = (
        await session.scalars(
            sa.update(Task)
            .where(
                Task.id == task_id,
                Task.claimed_by == worker_id,
                Task.attempt == attempt,
                Task.status.in_(LEASED_STATUSES),
            )
            .values(
                status=TaskStatus.CANCELLED,
                claimed_by=None,
                lease_expires_at=None,
                updated_at=sa.func.now(),
            )
            .returning(Task)
        )
    ).first()
    if task is None:
        return False
    await append_event(session, task.id, EventType.TASK_CANCELLED, {})
    await _try_wake_parent(session, task.parent_task_id)
    return True


async def reap_expired(session: AsyncSession, *, limit: int = 100) -> list[ReapedTask]:
    """Re-queue (or terminally fail) tasks whose lease expired.

    SKIP LOCKED keeps the reaper from ever blocking on a live worker's row;
    a worker that is merely slow will simply heartbeat and move on.
    """
    expired = (
        sa.select(Task.id)
        .where(Task.status.in_(LEASED_STATUSES), Task.lease_expires_at < sa.func.now())
        .limit(limit)
        .with_for_update(skip_locked=True)
        .cte("expired_tasks")
    )
    stmt = (
        sa.update(Task)
        .where(Task.id.in_(sa.select(expired.c.id)))
        .values(
            status=sa.case(
                (Task.attempt >= Task.max_attempts, TaskStatus.FAILED.value),
                else_=TaskStatus.PENDING.value,
            ),
            last_error=sa.case(
                (
                    Task.attempt >= Task.max_attempts,
                    "lease expired on final attempt (worker died?)",
                ),
                else_=Task.last_error,
            ),
            claimed_by=None,
            lease_expires_at=None,
            updated_at=sa.func.now(),
        )
        .returning(Task.id, Task.status, Task.attempt, Task.claimed_by, Task.parent_task_id)
    )
    rows = (await session.execute(stmt)).all()
    reaped: list[ReapedTask] = []
    for row in rows:
        item = ReapedTask(
            task_id=row.id, status=TaskStatus(row.status), attempt=row.attempt, claimed_by=None
        )
        reaped.append(item)
        await append_event(
            session,
            item.task_id,
            EventType.TASK_LEASE_EXPIRED,
            {"attempt": item.attempt, "requeued": item.status is TaskStatus.PENDING},
        )
        if item.status is TaskStatus.PENDING:
            await notify_task_ready(session, item.task_id)
        else:  # terminally failed by the reaper — its parent may be waiting
            await _try_wake_parent(session, row.parent_task_id)
        logger.info(
            "queue.task_reaped",
            task_id=str(item.task_id),
            attempt=item.attempt,
            new_status=item.status,
        )
    return reaped


async def _parent_of(session: AsyncSession, task_id: uuid.UUID) -> uuid.UUID | None:
    return await session.scalar(sa.select(Task.parent_task_id).where(Task.id == task_id))


async def _max_attempts_of(session: AsyncSession, task_id: uuid.UUID) -> int:
    value = await session.scalar(sa.select(Task.max_attempts).where(Task.id == task_id))
    return value if value is not None else 0


def _opt(value: uuid.UUID | None) -> str | None:
    return str(value) if value is not None else None
