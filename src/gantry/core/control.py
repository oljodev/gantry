"""The workspace emergency stop: read and trip the swarm-wide kill switch.

Per-task cancellation (even cascading down a subtree) answers "stop THIS run".
It does not answer the question an operator actually has at 3am — "stop
everything, now" — because that requires knowing every root task currently in
flight. This module is that switch: one durable flag per workspace that every
dispatcher consults before claiming.

The design mirrors the queue's existing posture on NOTIFY: the ``workspace_controls``
row is the truth, the notification is only a latency shortcut. A worker that
misses the NOTIFY (disconnected listener, fresh process) still observes the stop
within one refresh interval, and a worker that boots after the stop was tripped
observes it on its very first claim attempt.
"""

from __future__ import annotations

import uuid
from dataclasses import dataclass

import sqlalchemy as sa
from sqlalchemy.dialects.postgresql import insert as pg_insert
from sqlalchemy.ext.asyncio import AsyncSession

from gantry.core.models import WorkspaceControl
from gantry.core.notify import notify_workspace_control
from gantry.logging import get_logger

logger = get_logger(__name__)


@dataclass(frozen=True)
class ControlState:
    """A workspace's current stop state. Absent row == running."""

    workspace_id: uuid.UUID
    stopped: bool = False
    reason: str = ""
    actor: str = ""
    #: Spend ceiling in USD, or None to fall back to the deployment default.
    budget_usd: float | None = None

    @property
    def running(self) -> bool:
        return not self.stopped


async def get_control(session: AsyncSession, workspace_id: uuid.UUID) -> ControlState:
    """Read a workspace's stop state. A missing row means "never tripped".

    Deliberately a single indexed primary-key lookup: dispatchers call this on a
    timer while holding no other locks, so it must stay trivial even with a
    thousand agent slots on the process.
    """
    row = await session.get(WorkspaceControl, workspace_id)
    if row is None:
        return ControlState(workspace_id=workspace_id)
    return _state(row)


async def set_emergency_stop(
    session: AsyncSession,
    *,
    workspace_id: uuid.UUID,
    stopped: bool,
    reason: str = "",
    actor: str = "operator",
) -> ControlState:
    """Trip or clear the workspace's emergency stop, and wake every worker.

    Upserts so the first trip does not require the row to pre-exist (workspaces
    are implicit today), and NOTIFYs on commit so live dispatchers flip in the
    same instant rather than at their next refresh.
    """
    stmt = (
        pg_insert(WorkspaceControl)
        .values(
            workspace_id=workspace_id,
            stopped=stopped,
            reason=reason,
            actor=actor,
            stopped_at=sa.func.now() if stopped else None,
            updated_at=sa.func.now(),
        )
        .on_conflict_do_update(
            index_elements=[WorkspaceControl.workspace_id],
            set_={
                "stopped": stopped,
                "reason": reason,
                "actor": actor,
                "stopped_at": sa.func.now() if stopped else sa.null(),
                "updated_at": sa.func.now(),
            },
        )
        .returning(WorkspaceControl)
    )
    row = (await session.scalars(stmt)).one()
    await notify_workspace_control(session, workspace_id)
    logger.warning(
        "control.emergency_stop" if stopped else "control.emergency_stop_cleared",
        workspace_id=str(workspace_id),
        reason=reason,
        actor=actor,
    )
    return _state(row)


async def set_budget(
    session: AsyncSession,
    *,
    workspace_id: uuid.UUID,
    budget_usd: float | None,
) -> ControlState:
    """Set (or clear, with ``None``) a workspace's spend ceiling.

    Deliberately does NOT touch ``stopped``: raising the ceiling on a workspace
    the budget sentinel already halted leaves it halted until an operator
    explicitly clears the stop, so recovering from a runaway is always a
    conscious act rather than a side effect of editing a number.
    """
    stmt = (
        pg_insert(WorkspaceControl)
        .values(workspace_id=workspace_id, budget_usd=budget_usd, updated_at=sa.func.now())
        .on_conflict_do_update(
            index_elements=[WorkspaceControl.workspace_id],
            set_={"budget_usd": budget_usd, "updated_at": sa.func.now()},
        )
        .returning(WorkspaceControl)
    )
    row = (await session.scalars(stmt)).one()
    logger.info("control.budget_set", workspace_id=str(workspace_id), budget_usd=budget_usd)
    return _state(row)


def _state(row: WorkspaceControl) -> ControlState:
    return ControlState(
        workspace_id=row.workspace_id,
        stopped=bool(row.stopped),
        reason=row.reason,
        actor=row.actor,
        budget_usd=row.budget_usd,
    )
