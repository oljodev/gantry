from __future__ import annotations

import uuid
from typing import Any

import pytest
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.events import append_event, read_events
from gantry.core.models import DEFAULT_WORKSPACE_ID, EventType, Task, TaskKind
from gantry.runtime.loop import AgentLoopError, run_agent_task
from gantry.runtime.tools import INTERRUPTED_RESULT, ToolIdempotency, ToolRegistry, ToolResult

from .fakes import (
    CountingToolLLM,
    RecordingTool,
    ScriptedLLM,
    final_response,
    response_with_tool_call,
)

Sessions = async_sessionmaker[AsyncSession]


async def enqueue_agent_task(db: Sessions, payload: dict[str, Any] | None = None) -> Task:
    async with session_scope(db) as session:
        return await queue.enqueue(
            session,
            workspace_id=DEFAULT_WORKSPACE_ID,
            kind=TaskKind.EXECUTE,
            payload={"goal": "count things", "model": "fake/test", **(payload or {})},
        )


async def event_types(db: Sessions, task_id: uuid.UUID) -> list[EventType]:
    async with session_scope(db) as session:
        return [e.event_type for e in await read_events(session, task_id)]


async def test_happy_path_checkpoints_every_step(db: Sessions) -> None:
    task = await enqueue_agent_task(db)
    tool = RecordingTool()
    outcome = await run_agent_task(db, task, CountingToolLLM(2), ToolRegistry([tool]))

    assert outcome.final_text == "done after 2 increments"
    assert outcome.steps == 3
    assert not outcome.resumed
    assert tool.executions == [{"n": 1}, {"n": 2}]
    assert await event_types(db, task.id) == [
        EventType.TASK_ENQUEUED,
        EventType.LLM_REQUEST,
        EventType.LLM_RESPONSE,
        EventType.TOOL_CALL,
        EventType.TOOL_RESULT,
        EventType.LLM_REQUEST,
        EventType.LLM_RESPONSE,
        EventType.TOOL_CALL,
        EventType.TOOL_RESULT,
        EventType.LLM_REQUEST,
        EventType.LLM_RESPONSE,
    ]


async def test_tool_exception_becomes_error_result_the_llm_sees(db: Sessions) -> None:
    task = await enqueue_agent_task(db)
    tool = RecordingTool(fail_with=RuntimeError("disk on fire"))
    # After the tool error the completion guard nudges once (don't give up right
    # after a failure); the worker still finishes on the next no-tool-call turn.
    llm = ScriptedLLM(
        [
            response_with_tool_call("c1", "increment", {"n": 1}),
            final_response("still stuck"),
            final_response("gave up"),
        ]
    )
    outcome = await run_agent_task(db, task, llm, ToolRegistry([tool]))

    assert outcome.final_text == "gave up"
    # The second LLM call saw the error as a tool message.
    tool_msgs = [m for m in llm.calls[1]["messages"] if m["role"] == "tool"]
    assert len(tool_msgs) == 1 and "disk on fire" in tool_msgs[0]["content"]

    async with session_scope(db) as session:
        events = await read_events(session, task.id)
    result_events = [e for e in events if e.event_type is EventType.TOOL_RESULT]
    assert len(result_events) == 1 and result_events[0].payload["is_error"] is True


async def test_unknown_tool_becomes_error_result(db: Sessions) -> None:
    task = await enqueue_agent_task(db)
    # The unknown-tool error result trips the completion guard's one nudge before
    # the worker is allowed to finish.
    llm = ScriptedLLM(
        [
            response_with_tool_call("c1", "nonexistent", {}),
            final_response("retry"),
            final_response("ok"),
        ]
    )
    outcome = await run_agent_task(db, task, llm, ToolRegistry([]))
    assert outcome.final_text == "ok"
    tool_msgs = [m for m in llm.calls[1]["messages"] if m["role"] == "tool"]
    assert "Unknown tool" in tool_msgs[0]["content"]


async def _seed_crashed_step(
    db: Sessions, task: Task, *, started: bool, name: str = "increment"
) -> None:
    """Simulate a worker that died mid-step: response logged, result missing."""
    async with session_scope(db) as session:
        await append_event(
            session,
            task.id,
            EventType.LLM_RESPONSE,
            {
                "content": None,
                "tool_calls": [{"id": "c1", "name": name, "arguments": {"n": 1}}],
                "usage": {"prompt_tokens": 10, "completion_tokens": 5},
            },
        )
        if started:
            await append_event(
                session,
                task.id,
                EventType.TOOL_CALL,
                {"tool_call_id": "c1", "name": name, "arguments": {"n": 1}},
            )


async def test_resume_executes_unstarted_pending_tool_call(db: Sessions) -> None:
    task = await enqueue_agent_task(db)
    await _seed_crashed_step(db, task, started=False)

    tool = RecordingTool()
    llm = ScriptedLLM([final_response("resumed and finished")])
    outcome = await run_agent_task(db, task, llm, ToolRegistry([tool]))

    assert outcome.final_text == "resumed and finished"
    assert outcome.resumed
    assert tool.executions == [{"n": 1}]
    types = await event_types(db, task.id)
    assert types.count(EventType.TOOL_CALL) == 1
    assert types.count(EventType.TOOL_RESULT) == 1


async def test_resume_reruns_started_idempotent_tool(db: Sessions) -> None:
    task = await enqueue_agent_task(db)
    await _seed_crashed_step(db, task, started=True)

    tool = RecordingTool(idempotency=ToolIdempotency.IDEMPOTENT)
    llm = ScriptedLLM([final_response("done")])
    await run_agent_task(db, task, llm, ToolRegistry([tool]))

    assert tool.executions == [{"n": 1}]  # re-run is safe
    types = await event_types(db, task.id)
    assert types.count(EventType.TOOL_CALL) == 1  # no duplicate intent record


async def test_resume_non_idempotent_without_recovery_reports_interrupted(db: Sessions) -> None:
    task = await enqueue_agent_task(db)
    await _seed_crashed_step(db, task, started=True)

    tool = RecordingTool(idempotency=ToolIdempotency.NON_IDEMPOTENT)
    # The interrupted (error) result trips one completion nudge before finishing.
    llm = ScriptedLLM([final_response("checking"), final_response("acknowledged")])
    await run_agent_task(db, task, llm, ToolRegistry([tool]))

    assert tool.executions == []  # never blindly re-run
    assert tool.recover_calls == [{"n": 1}]
    tool_msgs = [m for m in llm.calls[0]["messages"] if m["role"] == "tool"]
    assert tool_msgs[0]["content"] == INTERRUPTED_RESULT.content


async def test_resume_non_idempotent_with_recovery_uses_recovered_result(db: Sessions) -> None:
    task = await enqueue_agent_task(db)
    await _seed_crashed_step(db, task, started=True)

    tool = RecordingTool(
        idempotency=ToolIdempotency.NON_IDEMPOTENT,
        recover_result=ToolResult(content="push landed: commit abc123"),
    )
    llm = ScriptedLLM([final_response("verified")])
    await run_agent_task(db, task, llm, ToolRegistry([tool]))

    assert tool.executions == []
    tool_msgs = [m for m in llm.calls[0]["messages"] if m["role"] == "tool"]
    assert tool_msgs[0]["content"] == "push landed: commit abc123"


async def test_max_steps_raises_instead_of_looping_forever(db: Sessions) -> None:
    task = await enqueue_agent_task(db, {"max_steps": 2})
    with pytest.raises(AgentLoopError, match="max_steps=2"):
        await run_agent_task(db, task, CountingToolLLM(100), ToolRegistry([RecordingTool()]))
