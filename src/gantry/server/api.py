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
from fastapi import APIRouter, Depends, HTTPException, Query, Request
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.config import Settings
from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.events import read_events
from gantry.core.models import (
    DEFAULT_PROJECT_ID,
    DEFAULT_WORKSPACE_ID,
    EventType,
    Project,
    Provider,
    Task,
    TaskEvent,
    TaskKind,
    TaskStatus,
)
from gantry.providers import resolve_model
from gantry.runtime.state import PLANNER_SYSTEM_PROMPT
from gantry.server.auth import require_user
from gantry.server.schemas import (
    ApprovalItem,
    ApprovalResolveRequest,
    ApprovalsResponse,
    ModelUsage,
    QuestionAnswerRequest,
    QuestionItem,
    QuestionsResponse,
    StatsResponse,
    TaskCreateRequest,
    TaskEventOut,
    TaskEventsResponse,
    TaskListResponse,
    TaskOut,
    UsagePoint,
    UsageResponse,
)
from gantry.worker.tools.orchestration import DEFAULT_PLANNER_MAX_ATTEMPTS

router = APIRouter(prefix="/api", tags=["tasks"], dependencies=[Depends(require_user)])

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
        if body.provider_id is not None:
            # Resolve to a final LiteLLM model string now so the runtime and
            # events only ever see one canonical model field.
            provider = await session.get(Provider, body.provider_id)
            if provider is None or provider.workspace_id != DEFAULT_WORKSPACE_ID:
                raise HTTPException(status_code=422, detail="unknown provider_id")
            settings = cast("Settings", request.app.state.settings)
            payload["model"] = resolve_model(provider, body.model, settings.default_model)
        project = await session.get(Project, body.project_id or DEFAULT_PROJECT_ID)
        if project is not None and project.auto_approve:
            payload["auto_approve"] = True
        task = await queue.enqueue(
            session,
            workspace_id=DEFAULT_WORKSPACE_ID,
            project_id=body.project_id,
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
    project_id: Annotated[uuid.UUID | None, Query()] = None,
    roots_only: Annotated[bool, Query()] = False,
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
    # A "run" is a root task; roots_only hides the spawned agents so the runs
    # list shows one row per launch (expand it to see the team's agents).
    if roots_only:
        stmt = stmt.where(Task.parent_task_id.is_(None))
    if project_id is not None:
        stmt = stmt.where(Task.project_id == project_id)
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
async def get_stats(
    request: Request,
    project_id: Annotated[uuid.UUID | None, Query()] = None,
) -> StatsResponse:
    sessions = get_sessions(request)
    project_filter = Task.project_id == project_id if project_id is not None else sa.true()
    async with sessions() as session:
        status_rows = (
            await session.execute(
                sa.select(Task.status, sa.func.count())
                .where(Task.workspace_id == DEFAULT_WORKSPACE_ID, project_filter)
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
                ).where(Task.result.isnot(None), project_filter)
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


#: Event types that carry an LLM ``usage`` block: model turns and the
#: summarizer call that compaction runs. Everything else is bookkeeping.
_USAGE_EVENTS = (EventType.LLM_RESPONSE.value, EventType.COMPACTION.value)


def _usage_sum(key: str) -> sa.ColumnElement[int]:
    """SUM of one integer field inside the event's JSON ``usage`` block,
    tolerating rows written before that field existed (NULL -> 0)."""
    field = TaskEvent.payload["usage"][key].astext
    return sa.func.coalesce(sa.func.sum(sa.cast(field, sa.BigInteger)), 0)


@router.get("/usage", response_model=UsageResponse)
async def get_usage(
    request: Request,
    project_id: Annotated[uuid.UUID | None, Query()] = None,
    days: Annotated[int, Query(ge=1, le=365)] = 30,
) -> UsageResponse:
    """Token usage aggregated from the event log — lifetime totals, a daily
    trend over the last ``days``, and a per-model breakdown."""
    sessions = get_sessions(request)
    project_filter = Task.project_id == project_id if project_id is not None else sa.true()
    scope = (
        Task.workspace_id == DEFAULT_WORKSPACE_ID,
        TaskEvent.event_type.in_(_USAGE_EVENTS),
        project_filter,
    )
    # A model turn counts as one "call"; the summarizer's compaction call does not.
    calls = sa.func.count().filter(TaskEvent.event_type == EventType.LLM_RESPONSE.value)
    joined = sa.join(TaskEvent, Task, Task.id == TaskEvent.task_id)

    async with sessions() as session:
        totals = (
            await session.execute(
                sa.select(
                    _usage_sum("prompt_tokens"),
                    _usage_sum("completion_tokens"),
                    _usage_sum("cache_read_tokens"),
                    _usage_sum("cache_write_tokens"),
                    calls,
                )
                .select_from(joined)
                .where(*scope)
            )
        ).one()

        # days is Query-validated to [1, 365], so this interpolation is safe.
        since = sa.func.now() - sa.text(f"interval '{days} days'")
        day = sa.func.date_trunc("day", TaskEvent.created_at)
        daily_rows = (
            await session.execute(
                sa.select(
                    day,
                    _usage_sum("prompt_tokens"),
                    _usage_sum("completion_tokens"),
                    _usage_sum("cache_read_tokens"),
                    calls,
                )
                .select_from(joined)
                .where(*scope, TaskEvent.created_at > since)
                .group_by(day)
                .order_by(day)
            )
        ).all()

        # Compaction rows have no model in payload -> attribute them to the summarizer.
        model = sa.func.coalesce(TaskEvent.payload["model"].astext, "summarizer")
        model_rows = (
            await session.execute(
                sa.select(
                    model,
                    _usage_sum("prompt_tokens"),
                    _usage_sum("completion_tokens"),
                    calls,
                )
                .select_from(joined)
                .where(*scope)
                .group_by(model)
                .order_by(sa.desc(_usage_sum("prompt_tokens") + _usage_sum("completion_tokens")))
            )
        ).all()

    return UsageResponse(
        prompt_tokens=int(totals[0]),
        completion_tokens=int(totals[1]),
        cache_read_tokens=int(totals[2]),
        cache_write_tokens=int(totals[3]),
        llm_calls=int(totals[4]),
        daily=[
            UsagePoint(
                date=row[0].date().isoformat(),
                prompt_tokens=int(row[1]),
                completion_tokens=int(row[2]),
                cache_read_tokens=int(row[3]),
                calls=int(row[4]),
            )
            for row in daily_rows
        ],
        by_model=[
            ModelUsage(
                model=str(row[0]),
                prompt_tokens=int(row[1]),
                completion_tokens=int(row[2]),
                calls=int(row[3]),
            )
            for row in model_rows
        ],
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


@router.get("/approvals", response_model=ApprovalsResponse)
async def list_approvals(
    request: Request,
    project_id: Annotated[uuid.UUID | None, Query()] = None,
) -> ApprovalsResponse:
    """The operator inbox: every task parked on an unresolved approval."""
    sessions = get_sessions(request)
    project_filter = Task.project_id == project_id if project_id is not None else sa.true()
    items: list[ApprovalItem] = []
    async with sessions() as session:
        waiting = (
            await session.scalars(
                sa.select(Task)
                .where(
                    Task.workspace_id == DEFAULT_WORKSPACE_ID,
                    Task.status == TaskStatus.WAITING_APPROVAL,
                    project_filter,
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


@router.get("/questions", response_model=QuestionsResponse)
async def list_questions(
    request: Request,
    project_id: Annotated[uuid.UUID | None, Query()] = None,
) -> QuestionsResponse:
    """The operator inbox for ask_user: every task parked on a question."""
    sessions = get_sessions(request)
    project_filter = Task.project_id == project_id if project_id is not None else sa.true()
    items: list[QuestionItem] = []
    async with sessions() as session:
        waiting = (
            await session.scalars(
                sa.select(Task)
                .where(
                    Task.workspace_id == DEFAULT_WORKSPACE_ID,
                    Task.status == TaskStatus.WAITING_INPUT,
                    project_filter,
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
                                EventType.ASK_USER_QUESTION.value,
                                EventType.ASK_USER_ANSWERED.value,
                            ]
                        ),
                    )
                    .order_by(TaskEvent.seq)
                )
            ).all()
            answered = {
                str(e.payload.get("tool_call_id"))
                for e in events
                if EventType(e.event_type) is EventType.ASK_USER_ANSWERED
            }
            for event in events:
                if (
                    EventType(event.event_type) is EventType.ASK_USER_QUESTION
                    and str(event.payload.get("tool_call_id")) not in answered
                ):
                    items.append(
                        QuestionItem(
                            task=TaskOut.model_validate(task),
                            request=TaskEventOut.model_validate(event),
                        )
                    )
    return QuestionsResponse(questions=items)


@router.post("/tasks/{task_id}/questions/{tool_call_id}", response_model=TaskOut)
async def answer_task_question(
    request: Request,
    task_id: uuid.UUID,
    tool_call_id: str,
    body: QuestionAnswerRequest,
) -> TaskOut:
    sessions = get_sessions(request)
    async with session_scope(sessions) as session:
        outcome, task = await queue.resolve_input(
            session,
            task_id=task_id,
            tool_call_id=tool_call_id,
            answer=body.answer,
            resolved_by=body.resolved_by,
        )
        if outcome == "resolved" and task is not None:
            return TaskOut.model_validate(task)
    if outcome == "already_resolved":
        raise HTTPException(status_code=409, detail="question already answered")
    raise HTTPException(status_code=404, detail="no such pending question")


@router.post("/tasks/{task_id}/cancel", response_model=TaskOut)
async def cancel_task(request: Request, task_id: uuid.UUID) -> TaskOut:
    sessions = get_sessions(request)
    async with session_scope(sessions) as session:
        result = await queue.cancel(session, task_id=task_id)
        if result is not None:
            return TaskOut.model_validate(result.task)
        task = await session.get(Task, task_id)
    if task is None:
        raise HTTPException(status_code=404, detail="task not found")
    raise HTTPException(
        status_code=409,
        detail=f"task is {task.status.value}; already finished",
    )
