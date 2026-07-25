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

#: The "Default" project every pre-projects row was backfilled into (migration
#: 0004). Runs/agents/teams are scoped by project_id; this is the fallback for
#: rows created without an explicit project (e.g. children inherit the parent's).
DEFAULT_PROJECT_ID = uuid.UUID("00000000-0000-0000-0000-000000000002")


class TaskStatus(enum.StrEnum):
    PENDING = "pending"
    CLAIMED = "claimed"
    RUNNING = "running"
    WAITING_APPROVAL = "waiting_approval"
    WAITING_INPUT = "waiting_input"
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
    # Orchestration (Phase 6): event-driven dormancy for planner tasks.
    TASK_PARKED = "task_parked"
    TASK_RESUMED = "task_resumed"
    # Agent runtime (Phase 2+) — declared now so the log schema is stable.
    LLM_REQUEST = "llm_request"
    LLM_RESPONSE = "llm_response"
    TOOL_CALL = "tool_call"
    #: Emitted for approval-gated calls only, immediately before execution —
    #: distinguishes "approved but never ran" (safe to run) from "approved and
    #: crashed mid-run" (idempotency policy applies) on resume.
    TOOL_STARTED = "tool_started"
    TOOL_RESULT = "tool_result"
    TERMINAL_CHUNK = "terminal_chunk"
    #: Live reasoning-token deltas from a thinking model (DeepSeek R1 etc.),
    #: streamed for display only — folded into no agent state on rehydration,
    #: exactly like terminal_chunk.
    REASONING_CHUNK = "reasoning_chunk"
    DIFF = "diff"
    COMPACTION = "compaction"
    APPROVAL_REQUESTED = "approval_requested"
    APPROVAL_RESOLVED = "approval_resolved"
    #: Human-in-the-loop questions (ask_user tool): the agent parks on a
    #: question and resumes with the operator's answer as the tool result.
    ASK_USER_QUESTION = "ask_user_question"
    ASK_USER_ANSWERED = "ask_user_answered"
    #: A spawning agent tried to finish while children it launched were still
    #: running; the loop injects a durable reminder and steers it to wait rather
    #: than orphaning them. Folds into history as a user message on rehydration.
    CHILDREN_PENDING = "children_pending"
    #: A durable nudge to keep an autonomous leader on the swarm workflow, folded
    #: into history as a user message on rehydration. Two kinds, by payload:
    #: survey-budget ({"surveyed": n}) pushes a leader that keeps reading without
    #: spawning to delegate now; landing ({"land": true}) stops a leader finishing
    #: with the integrated work still on a staging branch instead of on main.
    LEADER_NUDGE = "leader_nudge"
    #: Skills (Phase 8): full skill content pinned into the run's log.
    SKILL_INJECTED = "skill_injected"
    #: Co-pilot (Phase 5): a proposed skill/tree the UI stages for the user.
    COPILOT_PROPOSAL = "copilot_proposal"


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
    #: The project this run belongs to (Runs are scoped per project in the UI).
    project_id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        sa.ForeignKey("projects.id", ondelete="RESTRICT"),
        nullable=False,
        default=DEFAULT_PROJECT_ID,
    )
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
    #: Estimated USD this task spent (set once on its terminal transition). The
    #: run's spend is SUM(cost_usd) over a root_task_id — the per-run budget brake.
    cost_usd: Mapped[float] = mapped_column(
        sa.Float, nullable=False, server_default=sa.text("0"), default=0.0
    )

    attempt: Mapped[int] = mapped_column(sa.Integer, nullable=False, default=0)
    max_attempts: Mapped[int] = mapped_column(sa.Integer, nullable=False, default=3)

    claimed_by: Mapped[str | None] = mapped_column(sa.String(128), nullable=True)
    lease_expires_at: Mapped[datetime | None] = mapped_column(
        sa.DateTime(timezone=True), nullable=True
    )

    #: Set by the cancel endpoint on a live task; the owning worker reads it at
    #: its next heartbeat and aborts the run cooperatively (compare-and-set to
    #: CANCELLED). Pending/parked tasks are cancelled outright, never via this.
    cancel_requested: Mapped[bool] = mapped_column(
        sa.Boolean, nullable=False, server_default=sa.false(), default=False
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
        sa.Index("ix_tasks_project", "project_id"),
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


class Project(Base):
    """A top-level container grouping runs, agent profiles, and teams.

    Providers, GitHub, and auth stay account-global; only work artifacts are
    scoped per project. ``default_repo_url``/``default_base_branch`` prefill a
    project's launch forms.
    """

    __tablename__ = "projects"

    id: Mapped[uuid.UUID] = mapped_column(UUID(as_uuid=True), primary_key=True, default=uuid.uuid4)
    workspace_id: Mapped[uuid.UUID] = mapped_column(UUID(as_uuid=True), nullable=False)
    name: Mapped[str] = mapped_column(sa.String(100), nullable=False)
    description: Mapped[str] = mapped_column(sa.Text, nullable=False, default="")
    default_repo_url: Mapped[str | None] = mapped_column(sa.Text, nullable=True)
    default_base_branch: Mapped[str | None] = mapped_column(sa.String(200), nullable=True)
    #: HITL auto-accept: when on, gated tool calls in this project's runs are
    #: auto-approved (still recorded in the trace, never silent).
    auto_approve: Mapped[bool] = mapped_column(
        sa.Boolean, nullable=False, server_default=sa.false(), default=False
    )
    created_at: Mapped[datetime] = mapped_column(
        sa.DateTime(timezone=True), nullable=False, server_default=sa.func.now()
    )
    updated_at: Mapped[datetime] = mapped_column(
        sa.DateTime(timezone=True), nullable=False, server_default=sa.func.now()
    )

    __table_args__ = (
        sa.UniqueConstraint("workspace_id", "name", name="uq_projects_workspace_name"),
        sa.Index("ix_projects_workspace", "workspace_id"),
    )


class Skill(Base):
    """An authored skill: instructions injected into an agent's system prompt.

    Skills used to be read-only ``*.md`` files on disk; they now live here so
    they can be created and edited per project. ``body`` is the markdown
    appended to the prompt; ``match`` is the keyword list that auto-selects the
    skill when a goal mentions one of them.
    """

    __tablename__ = "skills"

    id: Mapped[uuid.UUID] = mapped_column(UUID(as_uuid=True), primary_key=True, default=uuid.uuid4)
    workspace_id: Mapped[uuid.UUID] = mapped_column(UUID(as_uuid=True), nullable=False)
    project_id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        sa.ForeignKey("projects.id", ondelete="RESTRICT"),
        nullable=False,
        default=DEFAULT_PROJECT_ID,
    )
    name: Mapped[str] = mapped_column(sa.String(100), nullable=False)
    description: Mapped[str] = mapped_column(sa.Text, nullable=False, default="")
    match: Mapped[list[str]] = mapped_column(JSONB, nullable=False, default=list)
    body: Mapped[str] = mapped_column(sa.Text, nullable=False, default="")
    created_at: Mapped[datetime] = mapped_column(
        sa.DateTime(timezone=True), nullable=False, server_default=sa.func.now()
    )
    updated_at: Mapped[datetime] = mapped_column(
        sa.DateTime(timezone=True), nullable=False, server_default=sa.func.now()
    )

    __table_args__ = (
        sa.UniqueConstraint("project_id", "name", name="uq_skills_project_name"),
        sa.Index("ix_skills_project", "project_id"),
    )


class CopilotSession(Base):
    """A saved co-pilot conversation, so a chat can be reopened and continued.

    Each ``turn`` is a ``{"user": str, "task_id": str}`` pair — the transcript
    itself lives in the referenced tasks' event logs (durable, event-sourced),
    so a session only needs to remember which tasks made up the conversation.
    """

    __tablename__ = "copilot_sessions"

    id: Mapped[uuid.UUID] = mapped_column(UUID(as_uuid=True), primary_key=True, default=uuid.uuid4)
    workspace_id: Mapped[uuid.UUID] = mapped_column(UUID(as_uuid=True), nullable=False)
    project_id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        sa.ForeignKey("projects.id", ondelete="CASCADE"),
        nullable=False,
        default=DEFAULT_PROJECT_ID,
    )
    #: "skill" or "tree".
    kind: Mapped[str] = mapped_column(sa.String(20), nullable=False)
    #: The team a tree co-pilot is scoped to (loose ref — the team may be gone).
    team_id: Mapped[uuid.UUID | None] = mapped_column(UUID(as_uuid=True), nullable=True)
    title: Mapped[str] = mapped_column(sa.Text, nullable=False, default="")
    #: [{"user": str, "task_id": str}, ...] in order.
    turns: Mapped[list[dict[str, str]]] = mapped_column(JSONB, nullable=False, default=list)
    created_at: Mapped[datetime] = mapped_column(
        sa.DateTime(timezone=True), nullable=False, server_default=sa.func.now()
    )
    updated_at: Mapped[datetime] = mapped_column(
        sa.DateTime(timezone=True), nullable=False, server_default=sa.func.now()
    )

    __table_args__ = (
        sa.Index("ix_copilot_sessions_project", "project_id"),
        sa.Index("ix_copilot_sessions_team", "team_id"),
    )


class WorkspaceControl(Base):
    """The workspace's emergency stop — the swarm-wide kill switch.

    One row per workspace, created on demand. When ``stopped`` is true every
    worker dispatcher refuses to claim new work and halts the tasks it is already
    running, so an operator can stop a runaway swarm (and its spend) without
    chasing individual task ids.

    This is durable control-plane state rather than an event: workers must be
    able to read "may I claim?" cheaply on every dispatch loop, and a worker that
    boots (or reconnects) after the stop was tripped must observe it immediately
    without replaying a log. NOTIFY on the control channel makes propagation
    instant; the row is what makes it *true*.
    """

    __tablename__ = "workspace_controls"

    workspace_id: Mapped[uuid.UUID] = mapped_column(UUID(as_uuid=True), primary_key=True)
    stopped: Mapped[bool] = mapped_column(
        sa.Boolean, nullable=False, server_default=sa.false(), default=False
    )
    #: Why the stop was tripped, shown in the dashboard and logged by workers.
    reason: Mapped[str] = mapped_column(sa.Text, nullable=False, default="")
    #: Who tripped it ("operator", an email, an automated guard).
    actor: Mapped[str] = mapped_column(sa.String(200), nullable=False, default="")
    #: When it was last tripped — NULL once cleared.
    stopped_at: Mapped[datetime | None] = mapped_column(sa.DateTime(timezone=True), nullable=True)
    updated_at: Mapped[datetime] = mapped_column(
        sa.DateTime(timezone=True), nullable=False, server_default=sa.func.now()
    )


class ProviderType(enum.StrEnum):
    """LLM provider families. ``LOCAL`` is any OpenAI-compatible endpoint."""

    OPENAI = "openai"
    ANTHROPIC = "anthropic"
    GOOGLE = "google"
    XAI = "xai"
    OPENROUTER = "openrouter"
    LOCAL = "local"


class Secret(Base):
    """Encrypted secret material — the ONLY table that ever holds credentials.

    ``ciphertext`` is ``nonce(12) || AES-256-GCM(plaintext)``; the key lives in
    ``GANTRY_VAULT_KEY`` outside the database. ``name`` is a per-workspace
    convention key: ``github:token``, ``provider:{provider_id}``. ``meta`` is
    for small NON-secret annotations (e.g. the GitHub login the token belongs
    to) so status endpoints never need to decrypt.
    """

    __tablename__ = "secrets"

    id: Mapped[uuid.UUID] = mapped_column(UUID(as_uuid=True), primary_key=True, default=uuid.uuid4)
    workspace_id: Mapped[uuid.UUID] = mapped_column(UUID(as_uuid=True), nullable=False)
    name: Mapped[str] = mapped_column(sa.String(128), nullable=False)
    ciphertext: Mapped[bytes] = mapped_column(sa.LargeBinary, nullable=False)
    last4: Mapped[str] = mapped_column(sa.String(4), nullable=False, default="")
    meta: Mapped[dict[str, Any]] = mapped_column(JSONB, nullable=False, default=dict)
    created_at: Mapped[datetime] = mapped_column(
        sa.DateTime(timezone=True), nullable=False, server_default=sa.func.now()
    )
    updated_at: Mapped[datetime] = mapped_column(
        sa.DateTime(timezone=True), nullable=False, server_default=sa.func.now()
    )

    __table_args__ = (
        sa.UniqueConstraint("workspace_id", "name", name="uq_secrets_workspace_name"),
        sa.Index("ix_secrets_workspace", "workspace_id"),
    )


class Provider(Base):
    """LLM provider configuration — non-secret fields only.

    The API key lives in ``secrets`` under ``provider:{id}``; this row keeps
    just ``api_key_last4`` for display.
    """

    __tablename__ = "providers"

    id: Mapped[uuid.UUID] = mapped_column(UUID(as_uuid=True), primary_key=True, default=uuid.uuid4)
    workspace_id: Mapped[uuid.UUID] = mapped_column(UUID(as_uuid=True), nullable=False)
    name: Mapped[str] = mapped_column(sa.String(100), nullable=False)
    provider_type: Mapped[ProviderType] = mapped_column(
        sa.Enum(
            ProviderType,
            name="provider_type",
            native_enum=False,
            create_constraint=False,
            length=32,
            values_callable=lambda e: [m.value for m in e],
        ),
        nullable=False,
    )
    #: Required for ``local`` (OpenAI-compatible endpoint); optional otherwise.
    base_url: Mapped[str | None] = mapped_column(sa.Text, nullable=True)
    default_model: Mapped[str] = mapped_column(sa.String(200), nullable=False)
    api_key_last4: Mapped[str] = mapped_column(sa.String(4), nullable=False, default="")
    created_at: Mapped[datetime] = mapped_column(
        sa.DateTime(timezone=True), nullable=False, server_default=sa.func.now()
    )
    updated_at: Mapped[datetime] = mapped_column(
        sa.DateTime(timezone=True), nullable=False, server_default=sa.func.now()
    )

    __table_args__ = (
        sa.UniqueConstraint("workspace_id", "name", name="uq_providers_workspace_name"),
        sa.Index("ix_providers_workspace", "workspace_id"),
    )


class AgentProfile(Base):
    """A reusable, named agent definition (prompt, model, permissions).

    Profiles are *templates*: launching a team snapshots the resolved profile
    into the task payload, so later edits never affect in-flight runs.
    """

    __tablename__ = "agent_profiles"

    id: Mapped[uuid.UUID] = mapped_column(UUID(as_uuid=True), primary_key=True, default=uuid.uuid4)
    workspace_id: Mapped[uuid.UUID] = mapped_column(UUID(as_uuid=True), nullable=False)
    project_id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        sa.ForeignKey("projects.id", ondelete="RESTRICT"),
        nullable=False,
        default=DEFAULT_PROJECT_ID,
    )
    #: The team that owns this agent. NULL -> an unassigned/draft agent (a
    #: project-level template not yet placed in a team). Each team has its OWN
    #: library, so deleting a team removes its agents (CASCADE).
    team_id: Mapped[uuid.UUID | None] = mapped_column(
        UUID(as_uuid=True), sa.ForeignKey("teams.id", ondelete="CASCADE"), nullable=True
    )
    name: Mapped[str] = mapped_column(sa.String(100), nullable=False)
    #: Short human description ("Senior coder", "Reviews diffs for bugs").
    role: Mapped[str] = mapped_column(sa.Text, nullable=False, default="")
    #: NULL -> the kind-appropriate default prompt at launch time.
    system_prompt: Mapped[str | None] = mapped_column(sa.Text, nullable=True)
    provider_id: Mapped[uuid.UUID | None] = mapped_column(
        UUID(as_uuid=True), sa.ForeignKey("providers.id", ondelete="SET NULL"), nullable=True
    )
    #: NULL -> provider.default_model -> settings.default_model.
    model: Mapped[str | None] = mapped_column(sa.String(200), nullable=True)
    max_steps: Mapped[int | None] = mapped_column(sa.Integer, nullable=True)
    can_spawn: Mapped[bool] = mapped_column(sa.Boolean, nullable=False, default=False)
    #: When on, this agent runs as an Autonomous Leader (swarm master): the
    #: leader system prompt is forced and delegation tools are unlocked even
    #: with no fixed children. Resolved into the launch snapshot, never mid-run.
    autonomous_leader: Mapped[bool] = mapped_column(sa.Boolean, nullable=False, default=False)
    #: An Autonomous Leader's model menu: [{"model": slug, "description": when to
    #: use it}]. Injected into the leader prompt and snapshotted so it can assign
    #: a cost-appropriate model per spawned worker. Only model slugs, never keys.
    model_options: Mapped[list[dict[str, Any]]] = mapped_column(JSONB, nullable=False, default=list)
    gated_tools: Mapped[list[str]] = mapped_column(JSONB, nullable=False, default=list)
    skills: Mapped[list[str]] = mapped_column(JSONB, nullable=False, default=list)
    created_at: Mapped[datetime] = mapped_column(
        sa.DateTime(timezone=True), nullable=False, server_default=sa.func.now()
    )
    updated_at: Mapped[datetime] = mapped_column(
        sa.DateTime(timezone=True), nullable=False, server_default=sa.func.now()
    )

    __table_args__ = (
        # Names are unique within a team, so two teams may each have a "coder";
        # unassigned (draft) agents are unique per project instead.
        sa.Index(
            "uq_agent_profiles_team_name",
            "team_id",
            "name",
            unique=True,
            postgresql_where=sa.text("team_id IS NOT NULL"),
        ),
        sa.Index(
            "uq_agent_profiles_project_name_draft",
            "project_id",
            "name",
            unique=True,
            postgresql_where=sa.text("team_id IS NULL"),
        ),
        sa.Index("ix_agent_profiles_workspace", "workspace_id"),
        sa.Index("ix_agent_profiles_project", "project_id"),
        sa.Index("ix_agent_profiles_team", "team_id"),
    )


class Team(Base):
    """A named tree of agent profiles, launched with a goal later."""

    __tablename__ = "teams"

    id: Mapped[uuid.UUID] = mapped_column(UUID(as_uuid=True), primary_key=True, default=uuid.uuid4)
    workspace_id: Mapped[uuid.UUID] = mapped_column(UUID(as_uuid=True), nullable=False)
    project_id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True),
        sa.ForeignKey("projects.id", ondelete="RESTRICT"),
        nullable=False,
        default=DEFAULT_PROJECT_ID,
    )
    name: Mapped[str] = mapped_column(sa.String(100), nullable=False)
    description: Mapped[str] = mapped_column(sa.Text, nullable=False, default="")
    created_at: Mapped[datetime] = mapped_column(
        sa.DateTime(timezone=True), nullable=False, server_default=sa.func.now()
    )
    updated_at: Mapped[datetime] = mapped_column(
        sa.DateTime(timezone=True), nullable=False, server_default=sa.func.now()
    )

    __table_args__ = (
        sa.UniqueConstraint("project_id", "name", name="uq_teams_project_name"),
        sa.Index("ix_teams_project", "project_id"),
    )


class TeamMember(Base):
    """Adjacency-list node of a team tree (NULL parent = root).

    Writes always replace a team's whole tree in one transaction, so no
    ordering/consistency subtleties arise. ``ON DELETE RESTRICT`` on the
    profile FK turns "delete a profile still used by a team" into a clean 409.
    """

    __tablename__ = "team_members"

    id: Mapped[uuid.UUID] = mapped_column(UUID(as_uuid=True), primary_key=True, default=uuid.uuid4)
    team_id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True), sa.ForeignKey("teams.id", ondelete="CASCADE"), nullable=False
    )
    parent_member_id: Mapped[uuid.UUID | None] = mapped_column(
        UUID(as_uuid=True), sa.ForeignKey("team_members.id", ondelete="CASCADE"), nullable=True
    )
    profile_id: Mapped[uuid.UUID] = mapped_column(
        UUID(as_uuid=True), sa.ForeignKey("agent_profiles.id", ondelete="RESTRICT"), nullable=False
    )
    position: Mapped[int] = mapped_column(sa.Integer, nullable=False, default=0)
    created_at: Mapped[datetime] = mapped_column(
        sa.DateTime(timezone=True), nullable=False, server_default=sa.func.now()
    )

    __table_args__ = (
        sa.Index("ix_team_members_team", "team_id"),
        # DB-enforced single root per team.
        sa.Index(
            "ux_team_members_root",
            "team_id",
            unique=True,
            postgresql_where=sa.text("parent_member_id IS NULL"),
        ),
    )
