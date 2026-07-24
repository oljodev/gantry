"""A leader that integrated its workers' branches must not report success while
the work is still only on a staging branch — the loop reminds it to land on main
(so the user doesn't have to merge the side branch by hand)."""

from __future__ import annotations

import uuid
from collections.abc import Sequence

from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core.db import session_scope
from gantry.core.events import read_events
from gantry.core.models import EventType
from gantry.runtime.llm import (
    DeltaSink,
    LLMResponse,
    LLMUsage,
    Message,
    ToolCallRequest,
    ToolSchema,
)
from gantry.runtime.loop import _MAX_LANDING_REMINDERS, run_agent_task
from gantry.runtime.tools import ToolRegistry

from .fakes import RecordingTool, final_response
from .test_agent_loop import enqueue_agent_task

Sessions = async_sessionmaker[AsyncSession]


def _leader_payload() -> dict[str, object]:
    return {"autonomous_leader": True, "can_spawn": True}


def _integration_tools() -> ToolRegistry:
    # Stand-ins: the guard only counts calls by name, not their effects.
    return ToolRegistry(
        [RecordingTool(name="merge_child_branches"), RecordingTool(name="land_branch")]
    )


async def _land_reminders(db: Sessions, task_id: uuid.UUID) -> int:
    async with session_scope(db) as session:
        events = await read_events(session, task_id)
    return sum(
        1 for e in events if e.event_type is EventType.LEADER_NUDGE and e.payload.get("land")
    )


def _call(name: str) -> LLMResponse:
    return LLMResponse(
        content=None,
        tool_calls=(ToolCallRequest(id=f"{name}_1", name=name, arguments={}),),
        usage=LLMUsage(prompt_tokens=10, completion_tokens=5),
    )


async def test_leader_is_reminded_to_land_before_finishing(db: Sessions) -> None:
    task = await enqueue_agent_task(db, _leader_payload())
    llm = _MergeThenFinishThenLandLLM()
    outcome = await run_agent_task(db, task, llm, _integration_tools())

    assert outcome.final_text == "landed on main"
    # Exactly one land reminder: it merged, tried to finish, got pushed to land.
    assert await _land_reminders(db, task.id) == 1
    assert any("never landed it on main" in str(m.get("content") or "") for m in llm.last_messages)


async def test_land_reminders_are_bounded(db: Sessions) -> None:
    task = await enqueue_agent_task(db, _leader_payload())
    # Merges once, then keeps trying to finish without ever landing.
    llm = _MergeThenAlwaysFinishLLM()
    outcome = await run_agent_task(db, task, llm, _integration_tools())

    assert outcome.final_text == "declaring done"
    assert await _land_reminders(db, task.id) == _MAX_LANDING_REMINDERS


async def test_no_reminder_when_nothing_was_integrated(db: Sessions) -> None:
    task = await enqueue_agent_task(db, _leader_payload())
    # A leader that never merged (e.g. delegated nothing) may finish freely.
    outcome = await run_agent_task(db, task, _JustFinishLLM(), _integration_tools())

    assert outcome.final_text == "nothing to integrate"
    assert await _land_reminders(db, task.id) == 0


class _MergeThenFinishThenLandLLM:
    """merge -> try to finish (gets nudged) -> land -> finish."""

    def __init__(self) -> None:
        self.last_messages: list[Message] = []

    async def complete(
        self,
        *,
        model: str,
        messages: list[Message],
        tools: Sequence[ToolSchema] = (),
        on_delta: DeltaSink | None = None,
    ) -> LLMResponse:
        self.last_messages = list(messages)
        called = {
            tc["function"]["name"]
            for m in messages
            if m.get("role") == "assistant"
            for tc in (m.get("tool_calls") or [])
        }
        if "merge_child_branches" not in called:
            return _call("merge_child_branches")
        if "land_branch" not in called:
            # If the reminder has arrived, land; otherwise (first attempt) try to
            # finish so the guard has a finish attempt to catch.
            if any("never landed it on main" in str(m.get("content") or "") for m in messages):
                return _call("land_branch")
            return final_response("all done")
        return final_response("landed on main")


class _MergeThenAlwaysFinishLLM:
    """Merges once, then always tries to finish — never lands."""

    async def complete(
        self,
        *,
        model: str,
        messages: list[Message],
        tools: Sequence[ToolSchema] = (),
        on_delta: DeltaSink | None = None,
    ) -> LLMResponse:
        merged = any(
            tc["function"]["name"] == "merge_child_branches"
            for m in messages
            if m.get("role") == "assistant"
            for tc in (m.get("tool_calls") or [])
        )
        if not merged:
            return _call("merge_child_branches")
        return final_response("declaring done")


class _JustFinishLLM:
    async def complete(
        self,
        *,
        model: str,
        messages: list[Message],
        tools: Sequence[ToolSchema] = (),
        on_delta: DeltaSink | None = None,
    ) -> LLMResponse:
        return final_response("nothing to integrate")
