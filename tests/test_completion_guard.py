"""A leaf worker must not be marked SUCCEEDED the instant it emits a no-tool-call
turn while its last action FAILED — the "gives up after 1-2 steps" bug where a
reasoning model narrates the fix it means to make, omits the tool call, and the
loop reads that as a finished run. The loop nudges it to keep going (bounded), and
never ends a run merely because a tool call returned or a step transitioned.
"""

from __future__ import annotations

import uuid
from typing import Any, ClassVar

from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core.db import session_scope
from gantry.core.events import read_events
from gantry.core.models import EventType
from gantry.runtime.loop import _MAX_COMPLETION_NUDGES, run_agent_task
from gantry.runtime.tools import Tool, ToolContext, ToolRegistry, ToolResult

from .fakes import ScriptedLLM, final_response, response_with_tool_call
from .test_agent_loop import enqueue_agent_task

Sessions = async_sessionmaker[AsyncSession]


class _MaybeFailTool(Tool):
    """A one-shot action that errors when called with ``fail: true``."""

    name = "run"
    description = "run a step"
    parameters: ClassVar[dict[str, Any]] = {
        "type": "object",
        "properties": {"fail": {"type": "boolean"}},
    }

    async def execute(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult:
        if arguments.get("fail"):
            return ToolResult("command failed: could not read Username", is_error=True)
        return ToolResult("ok")


def _registry() -> ToolRegistry:
    return ToolRegistry([_MaybeFailTool()])


async def _nudge_count(db: Sessions, task_id: uuid.UUID) -> int:
    async with session_scope(db) as session:
        events = await read_events(session, task_id)
    return sum(1 for e in events if e.event_type is EventType.COMPLETION_NUDGE)


async def test_worker_is_not_marked_done_after_a_failed_action_and_recovers(db: Sessions) -> None:
    task = await enqueue_agent_task(db)
    llm = ScriptedLLM(
        [
            response_with_tool_call("c1", "run", {"fail": True}),  # step 1: action fails
            final_response("the fetch failed, let me try a different way"),  # step 2: tries to quit
            response_with_tool_call("c2", "run", {"fail": False}),  # step 3: nudged -> recovers
            final_response("done for real"),  # step 4: clean finish
        ]
    )
    outcome = await run_agent_task(db, task, llm, _registry())

    # It did NOT stop at step 2 — the loop kept sending history back until a real
    # finish, and the final answer is the genuine one from step 4.
    assert outcome.final_text == "done for real"
    assert len(llm.calls) == 4
    assert await _nudge_count(db, task.id) == 1


async def test_a_clean_finish_is_not_nudged(db: Sessions) -> None:
    task = await enqueue_agent_task(db)
    llm = ScriptedLLM(
        [
            response_with_tool_call("c1", "run", {"fail": False}),  # succeeds
            final_response("done"),  # last action did not error -> genuine finish
        ]
    )
    outcome = await run_agent_task(db, task, llm, _registry())

    assert outcome.final_text == "done"
    assert len(llm.calls) == 2  # no extra turn
    assert await _nudge_count(db, task.id) == 0


async def test_completion_nudge_is_bounded(db: Sessions) -> None:
    task = await enqueue_agent_task(db)
    # A worker that fails once and then keeps trying to quit: nudged up to the cap,
    # then allowed to finish so a genuinely stuck worker still terminates.
    llm = ScriptedLLM(
        [
            response_with_tool_call("c1", "run", {"fail": True}),
            *[final_response(f"quit attempt {i}") for i in range(_MAX_COMPLETION_NUDGES + 1)],
        ]
    )
    outcome = await run_agent_task(db, task, llm, _registry())

    assert await _nudge_count(db, task.id) == _MAX_COMPLETION_NUDGES
    # One tool step + (cap + 1) finish attempts; the last one is allowed through.
    assert len(llm.calls) == _MAX_COMPLETION_NUDGES + 2
    assert outcome.final_text == f"quit attempt {_MAX_COMPLETION_NUDGES}"


async def test_a_multi_step_worker_stays_alive_across_tool_calls(db: Sessions) -> None:
    # The core guarantee: a tool call returning (a step transition) never ends the
    # run — only an explicit no-tool-call finish does.
    task = await enqueue_agent_task(db)
    llm = ScriptedLLM(
        [
            response_with_tool_call("c1", "run", {"fail": False}),
            response_with_tool_call("c2", "run", {"fail": False}),
            response_with_tool_call("c3", "run", {"fail": False}),
            final_response("finished after three steps"),
        ]
    )
    outcome = await run_agent_task(db, task, llm, _registry())

    assert outcome.final_text == "finished after three steps"
    assert len(llm.calls) == 4  # stayed alive through every step
    assert outcome.steps == 4
    assert await _nudge_count(db, task.id) == 0
