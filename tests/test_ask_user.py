"""ask_user HITL: park on a question, resume with the operator's answer.

Same durable park/resume machinery as approvals, exercised against real
Postgres via the ``db`` fixture.
"""

from __future__ import annotations

from typing import Any

import pytest
import sqlalchemy as sa
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.events import append_event
from gantry.core.models import (
    DEFAULT_WORKSPACE_ID,
    EventType,
    Task,
    TaskEvent,
    TaskKind,
    TaskStatus,
)
from gantry.runtime.tools import EventEmitter, TaskParked, ToolContext
from gantry.worker.tools.ask import AskUserTool

Sessions = async_sessionmaker[AsyncSession]


async def claimed_task(db: Sessions) -> Task:
    async with session_scope(db) as session:
        await queue.enqueue(
            session,
            workspace_id=DEFAULT_WORKSPACE_ID,
            kind=TaskKind.EXECUTE,
            payload={"goal": "do a thing"},
            max_attempts=20,
        )
    async with session_scope(db) as session:
        claimed = await queue.claim(session, worker_id="asker-w", lease_seconds=30)
    assert claimed is not None
    return claimed


def emitter_for(db: Sessions, task_id: Any) -> EventEmitter:
    async def _emit(event_type: EventType, payload: dict[str, Any]) -> int:
        async with session_scope(db) as session:
            return await append_event(session, task_id, event_type, payload)

    return _emit


def ctx_for(db: Sessions, task: Task, call_id: str) -> ToolContext:
    return ToolContext(
        task_id=task.id,
        sessions=db,
        emit_event=emitter_for(db, task.id),
        tool_call_id=call_id,
    )


async def status_of(db: Sessions, task_id: Any) -> TaskStatus:
    async with db() as session:
        status = await session.scalar(sa.select(Task.status).where(Task.id == task_id))
    assert status is not None
    return TaskStatus(status)


async def test_ask_user_parks_then_resumes_with_the_answer(db: Sessions) -> None:
    task = await claimed_task(db)
    ctx = ctx_for(db, task, "q1")

    # First execution: emits the question and parks.
    with pytest.raises(TaskParked) as parked:
        await AskUserTool().execute({"question": "Ship it?", "options": ["yes", "no"]}, ctx)
    assert parked.value.reason == TaskStatus.WAITING_INPUT.value

    # The question is durably in the log.
    async with db() as session:
        qs = (
            await session.scalars(
                sa.select(TaskEvent).where(
                    TaskEvent.task_id == task.id,
                    TaskEvent.event_type == EventType.ASK_USER_QUESTION.value,
                )
            )
        ).all()
    assert len(qs) == 1 and qs[0].payload["question"] == "Ship it?"

    # Park the task (worker side).
    async with session_scope(db) as session:
        status = await queue.park_for_input(
            session, task_id=task.id, worker_id="asker-w", attempt=task.attempt
        )
    assert status is TaskStatus.WAITING_INPUT

    # Operator answers → task re-queued.
    async with session_scope(db) as session:
        outcome, _ = await queue.resolve_input(
            session, task_id=task.id, tool_call_id="q1", answer="yes, ship it"
        )
    assert outcome == "resolved"
    assert await status_of(db, task.id) is TaskStatus.PENDING

    # Resume: re-run the same call — now it returns the answer, no re-park.
    async with session_scope(db) as session:
        resumed = await queue.claim(session, worker_id="asker-w2", lease_seconds=30)
    assert resumed is not None
    result = await AskUserTool().execute(
        {"question": "Ship it?", "options": ["yes", "no"]}, ctx_for(db, resumed, "q1")
    )
    assert not result.is_error and result.content == "yes, ship it"


async def test_answer_before_park_unparks_immediately(db: Sessions) -> None:
    """Lost-wakeup guard: an answer that lands before the park still resumes."""
    task = await claimed_task(db)
    with pytest.raises(TaskParked):
        await AskUserTool().execute({"question": "go?"}, ctx_for(db, task, "q1"))

    async with session_scope(db) as session:
        outcome, _ = await queue.resolve_input(
            session, task_id=task.id, tool_call_id="q1", answer="go"
        )
    assert outcome == "resolved"
    # Parking now must detect the answer and immediately re-queue.
    async with session_scope(db) as session:
        status = await queue.park_for_input(
            session, task_id=task.id, worker_id="asker-w", attempt=task.attempt
        )
    assert status is TaskStatus.PENDING


async def test_resolve_unknown_and_double_answer(db: Sessions) -> None:
    task = await claimed_task(db)
    with pytest.raises(TaskParked):
        await AskUserTool().execute({"question": "pick"}, ctx_for(db, task, "q1"))

    async with session_scope(db) as session:
        outcome, _ = await queue.resolve_input(
            session, task_id=task.id, tool_call_id="nope", answer="x"
        )
    assert outcome == "not_found"

    async with session_scope(db) as session:
        assert (await queue.resolve_input(session, task_id=task.id, tool_call_id="q1", answer="a"))[
            0
        ] == "resolved"
    async with session_scope(db) as session:
        assert (await queue.resolve_input(session, task_id=task.id, tool_call_id="q1", answer="b"))[
            0
        ] == "already_resolved"
