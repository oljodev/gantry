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
from gantry.core.models import DEFAULT_WORKSPACE_ID, Task, TaskStatus
from gantry.server.schemas import (
    TaskCreateRequest,
    TaskEventOut,
    TaskEventsResponse,
    TaskListResponse,
    TaskOut,
)

router = APIRouter(prefix="/api", tags=["tasks"])

Sessions = async_sessionmaker[AsyncSession]


def get_sessions(request: Request) -> Sessions:
    return cast("Sessions", request.app.state.sessions)


@router.post("/tasks", response_model=TaskOut, status_code=201)
async def create_task(request: Request, body: TaskCreateRequest) -> TaskOut:
    sessions = get_sessions(request)
    async with session_scope(sessions) as session:
        task = await queue.enqueue(
            session,
            workspace_id=DEFAULT_WORKSPACE_ID,
            kind=body.kind,
            payload=body.build_payload(),
            priority=body.priority,
            max_attempts=body.max_attempts,
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
