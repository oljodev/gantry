"""An autonomous leader must stop surveying and start spawning. A thinking model
will deliberate as long as it is allowed, so the loop budgets read-only survey
calls and injects a durable "delegate now" nudge once the leader reads too much
without spawning a single worker (the over-planning failure mode)."""

from __future__ import annotations

import uuid
from collections.abc import Sequence

import pytest
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core.db import session_scope
from gantry.core.events import read_events
from gantry.core.models import EventType
from gantry.runtime.llm import DeltaSink, LLMResponse, Message, ToolSchema
from gantry.runtime.loop import (
    _LEADER_SURVEY_BUDGET,
    _MAX_LEADER_NUDGES,
    AgentLoopError,
    _survey_budget_guard,
    run_agent_task,
)
from gantry.runtime.state import AgentState, TrackedMessage
from gantry.runtime.tools import ToolRegistry

from .fakes import RecordingTool, final_response, response_with_tool_call
from .test_agent_loop import enqueue_agent_task

Sessions = async_sessionmaker[AsyncSession]


def _leader_payload() -> dict[str, object]:
    # A real leader is a delegating agent (can_spawn) with the leader flag set.
    return {"autonomous_leader": True, "can_spawn": True}


async def _leader_nudge_events(db: Sessions, task_id: uuid.UUID) -> int:
    async with session_scope(db) as session:
        events = await read_events(session, task_id)
    return sum(1 for e in events if e.event_type is EventType.LEADER_NUDGE)


def _assistant_calling(name: str) -> Message:
    return {
        "role": "assistant",
        "content": None,
        "tool_calls": [
            {"id": "x", "type": "function", "function": {"name": name, "arguments": "{}"}}
        ],
    }


def test_count_tool_calls_reads_the_folded_history() -> None:
    state = AgentState(
        tracked=[
            TrackedMessage(None, {"role": "user", "content": "go"}),
            TrackedMessage(1, _assistant_calling("read_file")),
            TrackedMessage(2, _assistant_calling("grep")),
            TrackedMessage(3, _assistant_calling("read_file")),
        ]
    )
    assert state.count_tool_calls("read_file") == 2
    assert state.count_tool_calls("read_file", "grep") == 3
    assert state.count_tool_calls("spawn_subtask") == 0


async def test_guard_is_silent_until_the_survey_budget_is_crossed(db: Sessions) -> None:
    task = await enqueue_agent_task(db, _leader_payload())
    state = AgentState(tracked=[TrackedMessage(None, {"role": "user", "content": "go"})])
    # Just under budget: no nudge, no event.
    state.tracked += [
        TrackedMessage(i, _assistant_calling("read_file")) for i in range(_LEADER_SURVEY_BUDGET - 1)
    ]
    assert await _survey_budget_guard(db, task, state) is None
    assert await _leader_nudge_events(db, task.id) == 0


async def test_guard_stays_silent_once_the_leader_has_spawned(db: Sessions) -> None:
    task = await enqueue_agent_task(db, _leader_payload())
    state = AgentState(tracked=[TrackedMessage(None, {"role": "user", "content": "go"})])
    # Way over the survey budget, but it has already delegated — leave it alone.
    state.tracked += [
        TrackedMessage(i, _assistant_calling("read_file")) for i in range(_LEADER_SURVEY_BUDGET * 3)
    ]
    state.tracked.append(TrackedMessage(999, _assistant_calling("spawn_subtask")))
    assert await _survey_budget_guard(db, task, state) is None
    assert await _leader_nudge_events(db, task.id) == 0


async def test_leader_is_nudged_to_spawn_after_surveying_too_long(db: Sessions) -> None:
    task = await enqueue_agent_task(db, _leader_payload())
    llm = _SurveyUntilNudgedLLM()
    registry = ToolRegistry([RecordingTool(name="read_file"), RecordingTool(name="spawn_subtask")])
    outcome = await run_agent_task(db, task, llm, registry)

    assert outcome.final_text == "dispatched the batch"
    # Exactly one nudge: it surveyed to the budget, got pushed, and delegated.
    assert await _leader_nudge_events(db, task.id) == 1
    # The nudge reached the model and told it to stop and spawn.
    assert any("STOP surveying" in str(m.get("content") or "") for m in llm.last_messages)


async def test_nudges_are_bounded_for_a_leader_that_keeps_surveying(db: Sessions) -> None:
    task = await enqueue_agent_task(db, _leader_payload())
    # Reads far past every nudge threshold, ignoring each one, then tries to finish
    # having spawned nothing — the leader delivery gate now fails that empty exit.
    llm = _SurveyForeverLLM(reads=_LEADER_SURVEY_BUDGET * 3)
    with pytest.raises(AgentLoopError, match="without delegating"):
        await run_agent_task(db, task, llm, ToolRegistry([RecordingTool(name="read_file")]))

    # The nudges still fired (and were bounded) during the surveying before the gate.
    assert await _leader_nudge_events(db, task.id) == _MAX_LEADER_NUDGES


class _SurveyUntilNudgedLLM:
    """Surveys with read_file until it sees the 'delegate now' nudge, then
    reports it dispatched — the behaviour the guard is meant to produce."""

    def __init__(self) -> None:
        self.calls = 0
        self.last_messages: list[Message] = []
        self._spawned = False

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
        if self._spawned:
            return final_response("dispatched the batch")
        if any("STOP surveying" in str(m.get("content") or "") for m in messages):
            self._spawned = True  # obey the nudge: actually delegate
            return response_with_tool_call("spawn_1", "spawn_subtask", {"goal": "do the work"})
        n = sum(1 for m in messages if m.get("role") == "tool")
        return response_with_tool_call(f"read_{n + 1}", "read_file", {"path": f"f{n + 1}.py"})


class _SurveyForeverLLM:
    """Ignores the nudges and keeps reading until a fixed count, then answers."""

    def __init__(self, reads: int) -> None:
        self._reads = reads

    async def complete(
        self,
        *,
        model: str,
        messages: list[Message],
        tools: Sequence[ToolSchema] = (),
        on_delta: DeltaSink | None = None,
    ) -> LLMResponse:
        n = sum(1 for m in messages if m.get("role") == "tool")
        if n < self._reads:
            return response_with_tool_call(f"read_{n + 1}", "read_file", {"path": f"f{n + 1}.py"})
        return final_response("gave up surveying")
