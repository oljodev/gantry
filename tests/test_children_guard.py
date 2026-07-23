"""A spawning agent must not report success while children it launched are
still running — the loop nudges it to wait_for_children instead of orphaning
their work (the R1-tech-lead-finished-early bug)."""

from __future__ import annotations

import uuid
from collections.abc import Sequence

import sqlalchemy as sa
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.events import read_events
from gantry.core.models import DEFAULT_WORKSPACE_ID, EventType, Task, TaskKind, TaskStatus
from gantry.runtime.llm import DeltaSink, LLMResponse, Message, ToolSchema
from gantry.runtime.loop import _MAX_CHILDREN_REMINDERS, run_agent_task
from gantry.runtime.tools import ToolRegistry

from .fakes import final_response
from .test_agent_loop import enqueue_agent_task

Sessions = async_sessionmaker[AsyncSession]


async def _spawn_child(db: Sessions, parent: Task, *, status: TaskStatus) -> Task:
    async with session_scope(db) as session:
        child = await queue.enqueue(
            session,
            workspace_id=DEFAULT_WORKSPACE_ID,
            kind=TaskKind.EXECUTE,
            payload={"goal": "child work", "agent_name": "coder"},
            parent=parent,
        )
        if status is not TaskStatus.PENDING:
            await session.execute(sa.update(Task).where(Task.id == child.id).values(status=status))
        return child


async def _children_pending_events(db: Sessions, task_id: uuid.UUID) -> int:
    async with session_scope(db) as session:
        events = await read_events(session, task_id)
    return sum(1 for e in events if e.event_type is EventType.CHILDREN_PENDING)


async def test_agent_is_nudged_to_wait_while_a_child_runs(db: Sessions) -> None:
    task = await enqueue_agent_task(db)
    await _spawn_child(db, task, status=TaskStatus.RUNNING)

    # The agent keeps trying to finish; the child never settles in this test, so
    # the loop nudges it up to the cap and only then lets it finish.
    llm = _RepeatingLLM(final_response("all done"))
    outcome = await run_agent_task(db, task, llm, ToolRegistry([]))

    assert outcome.final_text == "all done"
    # Nudged exactly _MAX_CHILDREN_REMINDERS times, then allowed through.
    assert await _children_pending_events(db, task.id) == _MAX_CHILDREN_REMINDERS
    assert llm.calls == _MAX_CHILDREN_REMINDERS + 1
    # The reminder is in history and names the live child's agent.
    assert any("coder" in str(m.get("content")) for m in llm.last_messages)


async def test_agent_finishes_freely_once_children_are_terminal(db: Sessions) -> None:
    task = await enqueue_agent_task(db)
    await _spawn_child(db, task, status=TaskStatus.SUCCEEDED)

    llm = _RepeatingLLM(final_response("integrated their results"))
    outcome = await run_agent_task(db, task, llm, ToolRegistry([]))

    assert outcome.final_text == "integrated their results"
    assert await _children_pending_events(db, task.id) == 0
    assert llm.calls == 1  # finished on the first try, no nudge


class _RepeatingLLM:
    """Returns the same final response on every call — stands in for an agent
    that keeps trying to finish; records call count and the last messages seen."""

    def __init__(self, response: LLMResponse) -> None:
        self._response = response
        self.calls = 0
        self.last_messages: list[Message] = []

    async def complete(
        self,
        *,
        model: str,
        messages: list[Message],
        tools: Sequence[ToolSchema] = (),
        on_delta: DeltaSink | None = None,
    ) -> LLMResponse:
        self.calls += 1
        self.last_messages = list(messages)
        return self._response
