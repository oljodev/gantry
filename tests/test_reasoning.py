"""Thinking-model support: reasoning tokens stream live as REASONING_CHUNK
events, are recorded on the llm_response, and never leak into replayed history."""

from __future__ import annotations

from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core.db import session_scope
from gantry.core.events import read_events
from gantry.core.models import EventType
from gantry.runtime.loop import run_agent_task
from gantry.runtime.state import rehydrate
from gantry.runtime.tools import ToolRegistry

from .fakes import ScriptedLLM, final_response
from .test_agent_loop import enqueue_agent_task

Sessions = async_sessionmaker[AsyncSession]


async def test_reasoning_streams_live_and_is_recorded_but_not_replayed(db: Sessions) -> None:
    task = await enqueue_agent_task(db)
    llm = ScriptedLLM([final_response("the answer", reasoning="let me think about this carefully")])

    outcome = await run_agent_task(db, task, llm, ToolRegistry([]))
    assert outcome.final_text == "the answer"

    async with session_scope(db) as session:
        events = await read_events(session, task.id)

    # It streamed live: at least one REASONING_CHUNK landed, and concatenated
    # the chunks reproduce the model's thinking.
    chunks = [e.payload["data"] for e in events if e.event_type is EventType.REASONING_CHUNK]
    assert chunks
    assert "".join(chunks).strip() == "let me think about this carefully"

    # It is recorded on the llm_response for display.
    llm_response = next(e for e in events if e.event_type is EventType.LLM_RESPONSE)
    assert llm_response.payload["reasoning"] == "let me think about this carefully"

    # But it is NOT replayed into history — the assistant message carries only
    # the final content, per provider spec (drop reasoning from later turns).
    state = rehydrate(task.payload, events)
    assert state.messages[-1] == {"role": "assistant", "content": "the answer"}
    assert all("think" not in str(m.get("content")) for m in state.messages)


def test_split_reasoning_handles_native_tags_and_plain_content() -> None:
    from gantry.runtime.llm import _split_reasoning

    # Native reasoning_content is authoritative; content passes through.
    assert _split_reasoning("the answer", "deep thoughts") == ("the answer", "deep thoughts")

    # Inline <think> tags are extracted into reasoning and stripped from content.
    content, reasoning = _split_reasoning("<think>hmm let me see</think>the answer", "")
    assert content == "the answer" and reasoning == "hmm let me see"

    # A standard model's plain prose is returned untouched (no thinking view).
    assert _split_reasoning("just a normal answer", "") == ("just a normal answer", "")

    # Both sources combine, native first.
    content, reasoning = _split_reasoning("<think>b</think>ans", "a")
    assert content == "ans" and reasoning == "a\nb"


async def test_no_reasoning_means_no_reasoning_chunks(db: Sessions) -> None:
    task = await enqueue_agent_task(db)
    llm = ScriptedLLM([final_response("plain answer")])  # a non-thinking model

    await run_agent_task(db, task, llm, ToolRegistry([]))
    async with session_scope(db) as session:
        events = await read_events(session, task.id)
    assert not [e for e in events if e.event_type is EventType.REASONING_CHUNK]
    llm_response = next(e for e in events if e.event_type is EventType.LLM_RESPONSE)
    assert llm_response.payload["reasoning"] == ""
