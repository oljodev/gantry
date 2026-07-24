from __future__ import annotations

from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core.db import session_scope
from gantry.core.events import read_events
from gantry.core.models import EventType
from gantry.runtime.compaction import CompactionConfig
from gantry.runtime.llm import Message
from gantry.runtime.loop import run_agent_task
from gantry.runtime.state import rehydrate
from gantry.runtime.tools import ToolRegistry

from .fakes import RecordingTool, ScriptedLLM, final_response, response_with_tool_call
from .test_agent_loop import enqueue_agent_task

Sessions = async_sessionmaker[AsyncSession]


def count_messages(messages: list[Message]) -> int:
    return len(messages)


async def test_compaction_summarizes_and_rehydrates_identically(db: Sessions) -> None:
    task = await enqueue_agent_task(db)
    # Script: three tool steps, then (a summarize call), then the final answer.
    llm = ScriptedLLM(
        [
            response_with_tool_call("c1", "increment", {"n": 1}),
            response_with_tool_call("c2", "increment", {"n": 2}),
            response_with_tool_call("c3", "increment", {"n": 3}),
            final_response("SUMMARY-OF-EARLIER-WORK"),  # consumed by the summarizer
            final_response("all done"),
        ]
    )
    # Trigger once history exceeds 7 messages: sys,user + 3x(assistant,tool) = 8.
    config = CompactionConfig(
        max_context_tokens=7, keep_recent_messages=2, token_counter=count_messages
    )
    outcome = await run_agent_task(
        db, task, llm, ToolRegistry([RecordingTool()]), compaction=config
    )
    assert outcome.final_text == "all done"

    async with session_scope(db) as session:
        events = await read_events(session, task.id)
    compactions = [e for e in events if e.event_type is EventType.COMPACTION]
    assert len(compactions) == 1
    payload = compactions[0].payload
    assert payload["summary"] == "SUMMARY-OF-EARLIER-WORK"
    assert payload["summarized_messages"] == 6  # sys, user, 2x(assistant, tool)
    assert len(payload["kept_seqs"]) == 2  # third assistant + its tool result

    # The summarizer saw the old history plus the instruction, not the kept tail.
    summarize_call = llm.calls[3]
    contents = [str(m.get("content")) for m in summarize_call["messages"]]
    assert any("Summarize the conversation" in c for c in contents)
    assert summarize_call["tools"] == []

    # The final LLM call ran on the compacted history. The system prompt (0) and
    # the root goal (1) are untouchable anchors kept verbatim; the summary follows.
    final_call = llm.calls[4]
    roles = [m["role"] for m in final_call["messages"]]
    assert roles == ["system", "user", "user", "assistant", "tool"]
    assert final_call["messages"][1]["content"] == "count things"  # goal anchor survived
    assert "SUMMARY-OF-EARLIER-WORK" in final_call["messages"][2]["content"]

    # Crash-consistency: rehydrating from the log reproduces the same view.
    state = rehydrate(task.payload, events)
    assert [m["role"] for m in state.messages] == [
        "system",
        "user",
        "user",
        "assistant",
        "tool",
        "assistant",
    ]
    assert state.messages[1]["content"] == "count things"  # goal anchor
    assert "SUMMARY-OF-EARLIER-WORK" in state.messages[2]["content"]
    assert state.messages[-1]["content"] == "all done"
    assert state.pending_tool_calls() == []
