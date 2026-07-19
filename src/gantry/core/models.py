"""ORM models for the durable task queue.

Design notes:

- ``tasks`` rows are the *units of work*; ``task_events`` is the append-only
  log that makes execution durable (crash recovery = replay the log).
- ``attempt`` doubles as a **fencing token**: it is incremented atomically at
  claim time, and every mutation by a worker (heartbeat/complete/fail) must
  present the attempt it claimed. A zombie worker whose lease was reaped and
  whose task was re-claimed holds a stale attempt and its writes are rejected.
- Statuses are stored as plain VARCHAR (not native PG enums) so adding a
  status never requires ``ALTER TYPE``.
"""

from __future__ import annotations

import enum
import uuid
from datetime import datetime
from typing import Any

import sqlalchemy as sa
from sqlalchemy.dialects.postgresql import JSONB, UUID
from sqlalchemy.orm import Mapped, mapped_column

from gantry.core.db import Base

#: Placeholder tenant until workspaces are enforced in Phase 9. Every table is
#: tenancy-shaped from day 1; queries must always scope by workspace_id.
DEFAULT_WORKSPACE_ID = uuid.UUID("00000000-0000-0000-0000-000000000001")


class TaskStatus(enum.StrEnum):
    PENDING = "pending"
    CLAIMED = "claimed"
    RUNNING = "running"
    WAITING_APPROVAL = "waiting_approval"
    WAITING_CHILDREN = "waiting_children"
    SUCCEEDED = "succeeded"
    FAILED = "failed"
    CANCELLED = "cancelled"


#: Statuses in which a worker holds (or held) the task under a lease.
LEASED_STATUSES = (TaskStatus.CLAIMED, TaskStatus.RUNNING)

#: Terminal statuses — the queue never transitions a task out of these.
TERMINAL_STATUSES = (TaskStatus.SUCCEEDED, TaskStatus.FAILED, TaskStatus.CANCELLED)


class TaskKind(enum.StrEnum):
    PLAN = "plan"
    EXECUTE = "execute"


class EventType(enum.StrEnum):
    # Queue lifecycle (Phase 1)
    TASK_ENQUEUED = "task_enqueued"
    TASK_CLAIMED = "task_claimed"
    TASK_SUCCEEDED = "task_succeeded"
    TASK_FAILED = "task_failed"
    TASK_RETRY_SCHEDULED = "task_retry_scheduled"
    TASK_LEASE_EXPIRED = "task_lease_expired"
    TASK_CANCELLED = "task_cancelled"
    # Agent runtime (Phase 2+) — declared now so the log schema is stable.
    LLM_REQUEST = "llm_request"
    LLM_RESPONSE = "llm_response"
    TOOL_CALL = "tool_call"
    TOOL_RESULT = "tool_result"
    TERMINAL_CHUNK = "terminal_chunk"
    DIFF = "diff"
    COMPACTION = "compaction"
    APPROVAL_REQUESTED = "approval_requested"
    APPROVAL_RESOLVED = "approval_resolved"


def _status_column() -> sa.Enum:
    return sa.Enum(
        TaskStatus,
        name="task_status",
        native_enum=False,
        create_constraint=False,
        length=32,
        values_callable=lambda e: [m.value for m in e],
    )


class Task(Base):
    __tablename__ = "tasks"

    id: Mapped[uuid.UUID] = mapped_column(UUID(as_uuid=True), primary_key=True, default=uuid.uuid4)
    workspace_id: Mapped[uuid.UUID] = mapped_column(UUID(as_uuid=True), nullable=False)
    parent_task_id: Mapped[uuid.UUID | None] = mapped_column(
        UUID(as_uuid=True), sa.ForeignKey("tasks.id", ondelete="CASCADE"), nullable=True
    )
    #: Root of this task's tree (== id for roots). Lets the UI load a whole
    #: trace tree with one indexed query instead of a recursive CTE.
    root_task_id: Mapped[uuid.UUID] = mapped_column(UUID(as_uuid=True), nullable=False)

    kind: Mapped[TaskKind] = mapped_column(
        sa.Enum(
            TaskKind,
            name="task_kind",
            native_enum=False,
            create_constraint=False,
            length=32,
            values_callable=lambda e: [m.value for m in e],
        ),
        nullable=False,
    )
    status: Mapped[TaskStatus] = mapped_column(
        _status_column(), nullable=False, default=TaskStatus.PENDING
    )
    priority: Mapped[int] = mapped_column(sa.Integer, nullable=False, default=0)

    payload: Mapped[dict[str, Any]] = mapped_column(JSONB, nullable=False, default=dict)
    result: Mapped[dict[str, Any] | None] = mapped_column(JSONB, nullable=True)
    last_error: Mapped[str | None] = mapped_column(sa.Text, nullable=True)

    attempt: Mapped[int] = mapped_column(sa.Integer, nullable=False, default=0)
    max_attempts: Mapped[int] = mapped_column(sa.Integer, nullable=False, default=3)

    claimed_by: Mapped[str | None] = mapped_column(sa.String(128), nullable=True)
    lease_expires_at: Mapped[datetime | None] = mapped_column(
        sa.DateTime(timezone=True), nullable=True
    )

    #: Earliest moment the task may be claimed (used for retry backoff).
    scheduled_at: Mapped[datetime] = mapped_column(
        sa.DateTime(timezone=True), nullable=False, server_default=sa.func.now()
    )
    created_at: Mapped[datetime] = mapped_column(
        sa.DateTime(timezone=True), nullable=False, server_default=sa.func.now()
    )
    updated_at: Mapped[datetime] = mapped_column(
        sa.DateTime(timezone=True), nullable=False, server_default=sa.func.now()
    )

    __table_args__ = (
        # Claim path: one index-only scan finds the next runnable task.
        sa.Index(
            "ix_tasks_claimable",
            sa.text("priority DESC"),
            "scheduled_at",
            postgresql_where=sa.text("status = 'pending'"),
        ),
        # Reaper path: expired leases only.
        sa.Index(
            "ix_tasks_lease_expiry",
            "lease_expires_at",
            postgresql_where=sa.text("status IN ('claimed', 'running')"),
        ),
        sa.Index("ix_tasks_workspace", "workspace_id"),
        sa.Index("ix_tasks_parent", "parent_task_id"),
        sa.Index("ix_tasks_root", "root_task_id"),
    )


class TaskEvent(Base):
    __tablename__ = "task_events"

    id: Mapped[int] = mapped_column(sa.BigInteger, sa.Identity(), primary_key=True)
    task_id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True), sa.ForeignKey("tasks.id", ondelete="CASCADE"), nullable=False
    )
    #: Per-task monotonic sequence — the replay order for resume. Enforced
    #: unique so two writers can never interleave ambiguously.
    seq: Mapped[int] = mapped_column(sa.Integer, nullable=False)
    event_type: Mapped[EventType] = mapped_column(
        sa.Enum(
            EventType,
            name="event_type",
            native_enum=False,
            create_constraint=False,
            length=32,
            values_callable=lambda e: [m.value for m in e],
        ),
        nullable=False,
    )
    payload: Mapped[dict[str, Any]] = mapped_column(JSONB, nullable=False, default=dict)
    created_at: Mapped[datetime] = mapped_column(
        sa.DateTime(timezone=True), nullable=False, server_default=sa.func.now()
    )

    # The unique constraint's backing index also serves ordered per-task reads.
    __table_args__ = (sa.UniqueConstraint("task_id", "seq", name="uq_task_events_task_seq"),)
