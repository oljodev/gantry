"""Pure rehydration tests — no database needed; events are built in memory."""

from __future__ import annotations

import uuid
from typing import Any

from gantry.core.models import EventType, TaskEvent
from gantry.runtime.state import rehydrate

TASK_ID = uuid.uuid4()


def event(seq: int, event_type: EventType, payload: dict[str, Any]) -> TaskEvent:
    return TaskEvent(task_id=TASK_ID, seq=seq, event_type=event_type, payload=payload)


def llm_response(
    seq: int, content: str | None, tool_calls: list[dict[str, Any]] | None = None
) -> TaskEvent:
    return event(
        seq,
        EventType.LLM_RESPONSE,
        {
            "content": content,
            "tool_calls": tool_calls or [],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5},
        },
    )


PAYLOAD = {"goal": "count to two", "system_prompt": "You are a test agent."}


def test_fresh_task_has_only_initial_messages() -> None:
    state = rehydrate(PAYLOAD, [])
    assert [m["role"] for m in state.messages] == ["system", "user"]
    assert state.messages[0]["content"] == "You are a test agent."
    assert state.messages[1]["content"] == "count to two"
    assert not state.resumed
    assert state.steps == 0
    assert state.pending_tool_calls() == []


def test_fold_full_step_history() -> None:
    events = [
        event(1, EventType.TASK_ENQUEUED, {}),
        llm_response(2, None, [{"id": "c1", "name": "increment", "arguments": {"n": 1}}]),
        event(3, EventType.TOOL_CALL, {"tool_call_id": "c1", "name": "increment"}),
        event(
            4,
            EventType.TOOL_RESULT,
            {"tool_call_id": "c1", "name": "increment", "content": "ok", "is_error": False},
        ),
        llm_response(5, "all done"),
    ]
    state = rehydrate(PAYLOAD, events)
    assert [m["role"] for m in state.messages] == [
        "system",
        "user",
        "assistant",
        "tool",
        "assistant",
    ]
    assert state.steps == 2
    assert state.resumed
    assert state.prompt_tokens == 20 and state.completion_tokens == 10
    assert state.pending_tool_calls() == []
    assert state.messages[-1]["content"] == "all done"


def test_pending_detection_distinguishes_started_from_unstarted() -> None:
    events = [
        llm_response(
            1,
            None,
            [
                {"id": "c1", "name": "a", "arguments": {}},
                {"id": "c2", "name": "b", "arguments": {}},
                {"id": "c3", "name": "c", "arguments": {}},
            ],
        ),
        event(2, EventType.TOOL_CALL, {"tool_call_id": "c1", "name": "a"}),
        event(
            3,
            EventType.TOOL_RESULT,
            {"tool_call_id": "c1", "name": "a", "content": "ok", "is_error": False},
        ),
        # c2: started (TOOL_CALL logged) but crashed before its result.
        event(4, EventType.TOOL_CALL, {"tool_call_id": "c2", "name": "b"}),
        # c3: never started.
    ]
    state = rehydrate(PAYLOAD, events)
    pending = state.pending_tool_calls()
    assert [tc.id for tc in pending] == ["c2", "c3"]
    assert "c2" in state.started_tool_ids
    assert "c3" not in state.started_tool_ids


def test_compaction_event_folds_identically() -> None:
    events = [
        llm_response(1, None, [{"id": "c1", "name": "a", "arguments": {}}]),
        event(
            2,
            EventType.TOOL_RESULT,
            {"tool_call_id": "c1", "name": "a", "content": "r1", "is_error": False},
        ),
        llm_response(3, None, [{"id": "c2", "name": "a", "arguments": {}}]),
        event(
            4,
            EventType.TOOL_RESULT,
            {"tool_call_id": "c2", "name": "a", "content": "r2", "is_error": False},
        ),
        # Compaction kept only the second step (seqs 3, 4).
        event(
            5,
            EventType.COMPACTION,
            {"summary": "did step one", "kept_seqs": [3, 4], "usage": {}},
        ),
        llm_response(6, "finished"),
    ]
    state = rehydrate(PAYLOAD, events)
    roles = [m["role"] for m in state.messages]
    # System (0) and the goal (1) are untouchable anchors; the summary follows.
    assert roles == ["system", "user", "user", "assistant", "tool", "assistant"]
    assert state.messages[1]["content"] == "count to two"  # goal anchor survived
    assert "did step one" in state.messages[2]["content"]
    # Kept tail is the second step, not the first.
    assert state.messages[4]["content"] == "r2"
    assert state.messages[-1]["content"] == "finished"
