"""REST surface of the control plane.

Thin by design: every route is a projection over ``tasks``/``task_events`` or
a call into ``gantry.core.queue`` — the API owns no state machine of its own,
so the invariants proven in Phase 1 hold no matter which door a mutation
comes through.
"""

from __future__ import annotations

import uuid
from typing import Annotated, cast

import sqlalchemy as sa
from fastapi import APIRouter, HTTPException, Query, Request
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.events import read_events
from gantry.core.models import (
    DEFAULT_WORKSPACE_ID,
    EventType,
    Task,
    TaskEvent,
    TaskKind,
    TaskStatus,
)
from gantry.runtime.state import PLANNER_SYSTEM_PROMPT
from gantry.server.schemas import (
    ApprovalItem,
    ApprovalResolveRequest,
    ApprovalsResponse,
    SkillOut,
    SkillsResponse,
    StatsResponse,
    TaskCreateRequest,
    TaskEventOut,
    TaskEventsResponse,
    TaskListResponse,
    TaskOut,
)
from gantry.skills import SkillRegistry
from gantry.worker.tools.orchestration import DEFAULT_PLANNER_MAX_ATTEMPTS

router = APIRouter(prefix="/api", tags=["tasks"])

Sessions = async_sessionmaker[AsyncSession]


def get_sessions(request: Request) -> Sessions:
    return cast("Sessions", request.app.state.sessions)


@router.post("/tasks", response_model=TaskOut, status_code=201)
async def create_task(request: Request, body: TaskCreateRequest) -> TaskOut:
    sessions = get_sessions(request)
    payload = body.build_payload()
    max_attempts = body.max_attempts
    if body.kind is TaskKind.PLAN:
        payload.setdefault("system_prompt", PLANNER_SYSTEM_PROMPT)
        if "max_attempts" not in body.model_fields_set:
            # Every park/wake cycle consumes an attempt (the fencing token).
            max_attempts = DEFAULT_PLANNER_MAX_ATTEMPTS
    async with session_scope(sessions) as session:
        task = await queue.enqueue(
            session,
            workspace_id=DEFAULT_WORKSPACE_ID,
            kind=body.kind,
            payload=payload,
            priority=body.priority,
            max_attempts=max_attempts,
        )
    return TaskOut.model_validate(task)


@router.get("/tasks", response_model=TaskListResponse)
async def list_tasks(
    request: Request,
    status: Annotated[TaskStatus | None, Query()] = None,
    root_task_id: Annotated[uuid.UUID | None, Query()] = None,
    limit: Annotated[int, Query(ge=1, le=200)] = 50,
    offset: Annotated[int, Query(ge=0)] = 0,
) -> TaskListResponse:
    sessions = get_sessions(request)
    stmt = (
        sa.select(Task)
        .where(Task.workspace_id == DEFAULT_WORKSPACE_ID)
        .order_by(Task.created_at.desc(), Task.id)
        .limit(limit)
        .offset(offset)
    )
    if status is not None:
        stmt = stmt.where(Task.status == status)
    if root_task_id is not None:
        stmt = stmt.where(Task.root_task_id == root_task_id)
    async with sessions() as session:
        tasks = (await session.scalars(stmt)).all()
    return TaskListResponse(tasks=[TaskOut.model_validate(t) for t in tasks])


@router.get("/tasks/{task_id}", response_model=TaskOut)
async def get_task(request: Request, task_id: uuid.UUID) -> TaskOut:
    sessions = get_sessions(request)
    async with sessions() as session:
        task = await session.get(Task, task_id)
    if task is None:
        raise HTTPException(status_code=404, detail="task not found")
    return TaskOut.model_validate(task)


@router.get("/tasks/{task_id}/events", response_model=TaskEventsResponse)
async def get_task_events(
    request: Request,
    task_id: uuid.UUID,
    after_seq: Annotated[int, Query(ge=0)] = 0,
    limit: Annotated[int, Query(ge=1, le=5000)] = 1000,
) -> TaskEventsResponse:
    sessions = get_sessions(request)
    async with sessions() as session:
        if await session.get(Task, task_id) is None:
            raise HTTPException(status_code=404, detail="task not found")
        events = await read_events(session, task_id, after_seq=after_seq, limit=limit)
    return TaskEventsResponse(events=[TaskEventOut.model_validate(e) for e in events])


@router.get("/stats", response_model=StatsResponse)
async def get_stats(request: Request) -> StatsResponse:
    sessions = get_sessions(request)
    async with sessions() as session:
        status_rows = (
            await session.execute(
                sa.select(Task.status, sa.func.count())
                .where(Task.workspace_id == DEFAULT_WORKSPACE_ID)
                .group_by(Task.status)
            )
        ).all()
        active_workers = (
            await session.scalar(
                sa.select(sa.func.count(sa.func.distinct(Task.claimed_by))).where(
                    Task.claimed_by.isnot(None)
                )
            )
            or 0
        )
        recent_workers = (
            await session.scalar(
                sa.select(
                    sa.func.count(sa.func.distinct(TaskEvent.payload["worker_id"].astext))
                ).where(
                    TaskEvent.event_type == EventType.TASK_CLAIMED.value,
                    TaskEvent.created_at > sa.func.now() - sa.text("interval '15 minutes'"),
                )
            )
            or 0
        )
        token_rows = (
            await session.execute(
                sa.select(
                    sa.func.coalesce(
                        sa.func.sum(sa.cast(Task.result["prompt_tokens"].astext, sa.BigInteger)), 0
                    ),
                    sa.func.coalesce(
                        sa.func.sum(
                            sa.cast(Task.result["completion_tokens"].astext, sa.BigInteger)
                        ),
                        0,
                    ),
                ).where(Task.result.isnot(None))
            )
        ).one()
        events_last_hour = (
            await session.scalar(
                sa.select(sa.func.count())
                .select_from(TaskEvent)
                .where(TaskEvent.created_at > sa.func.now() - sa.text("interval '1 hour'"))
            )
            or 0
        )
    statuses = {TaskStatus(row[0]).value: row[1] for row in status_rows}
    return StatsResponse(
        total=sum(statuses.values()),
        statuses=statuses,
        active_workers=active_workers,
        recent_workers=recent_workers,
        prompt_tokens=int(token_rows[0]),
        completion_tokens=int(token_rows[1]),
        events_last_hour=events_last_hour,
    )


@router.post("/tasks/{task_id}/retry", response_model=TaskOut)
async def retry_task(request: Request, task_id: uuid.UUID) -> TaskOut:
    """Re-queue a failed/cancelled task; it resumes from its event log."""
    sessions = get_sessions(request)
    async with session_scope(sessions) as session:
        retried = await queue.retry(session, task_id=task_id)
        if retried is not None:
            return TaskOut.model_validate(retried)
        task = await session.get(Task, task_id)
    if task is None:
        raise HTTPException(status_code=404, detail="task not found")
    raise HTTPException(
        status_code=409,
        detail=f"task is {task.status.value}; only failed or cancelled tasks can be retried",
    )


@router.get("/skills", response_model=SkillsResponse)
async def list_skills(request: Request) -> SkillsResponse:
    registry = cast("SkillRegistry", request.app.state.skills)
    return SkillsResponse(
        skills=[
            SkillOut(name=s.name, description=s.description, match=list(s.match))
            for s in registry.all()
        ]
    )


@router.get("/approvals", response_model=ApprovalsResponse)
async def list_approvals(request: Request) -> ApprovalsResponse:
    """The operator inbox: every task parked on an unresolved approval."""
    sessions = get_sessions(request)
    items: list[ApprovalItem] = []
    async with sessions() as session:
        waiting = (
            await session.scalars(
                sa.select(Task)
                .where(
                    Task.workspace_id == DEFAULT_WORKSPACE_ID,
                    Task.status == TaskStatus.WAITING_APPROVAL,
                )
                .order_by(Task.updated_at)
            )
        ).all()
        for task in waiting:
            events = (
                await session.scalars(
                    sa.select(TaskEvent)
                    .where(
                        TaskEvent.task_id == task.id,
                        TaskEvent.event_type.in_(
                            [
                                EventType.APPROVAL_REQUESTED.value,
                                EventType.APPROVAL_RESOLVED.value,
                            ]
                        ),
                    )
                    .order_by(TaskEvent.seq)
                )
            ).all()
            resolved = {
                str(e.payload.get("tool_call_id"))
                for e in events
                if EventType(e.event_type) is EventType.APPROVAL_RESOLVED
            }
            for event in events:
                if (
                    EventType(event.event_type) is EventType.APPROVAL_REQUESTED
                    and str(event.payload.get("tool_call_id")) not in resolved
                ):
                    items.append(
                        ApprovalItem(
                            task=TaskOut.model_validate(task),
                            request=TaskEventOut.model_validate(event),
                        )
                    )
    return ApprovalsResponse(approvals=items)


@router.post("/tasks/{task_id}/approvals/{tool_call_id}", response_model=TaskOut)
async def resolve_task_approval(
    request: Request,
    task_id: uuid.UUID,
    tool_call_id: str,
    body: ApprovalResolveRequest,
) -> TaskOut:
    sessions = get_sessions(request)
    async with session_scope(sessions) as session:
        outcome, task = await queue.resolve_approval(
            session,
            task_id=task_id,
            tool_call_id=tool_call_id,
            approved=body.decision == "approved",
            comment=body.comment,
            resolved_by=body.resolved_by,
        )
        if outcome == "resolved" and task is not None:
            return TaskOut.model_validate(task)
    if outcome == "already_resolved":
        raise HTTPException(status_code=409, detail="approval already resolved")
    raise HTTPException(status_code=404, detail="no such pending approval")


@router.post("/tasks/{task_id}/cancel", response_model=TaskOut)
async def cancel_task(request: Request, task_id: uuid.UUID) -> TaskOut:
    sessions = get_sessions(request)
    async with session_scope(sessions) as session:
        cancelled = await queue.cancel(session, task_id=task_id)
        if cancelled is not None:
            return TaskOut.model_validate(cancelled)
        task = await session.get(Task, task_id)
    if task is None:
        raise HTTPException(status_code=404, detail="task not found")
    raise HTTPException(
        status_code=409,
        detail=f"task is {task.status.value}; only pending tasks can be cancelled",
    )
