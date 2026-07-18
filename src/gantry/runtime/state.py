"""Agent state reconstruction: the event log *is* the agent's memory.

``rehydrate()`` folds a task's ``task_events`` into the exact OpenAI-style
message history the loop would hold in RAM — so any process can pick up any
task at any point. Initial messages (system prompt + goal) are derived
deterministically from the task payload; runtime events layer deltas on top:

- ``llm_response``  → assistant message (content and/or tool_calls)
- ``tool_result``   → tool message
- ``tool_call``     → marks a call as *started* (execution may have happened)
- ``compaction``    → collapses history to [system, summary] + the messages
  whose seqs were recorded in ``kept_seqs`` at compaction time
"""

from __future__ import annotations

import json
from collections.abc import Sequence
from dataclasses import dataclass, field
from typing import Any

from gantry.core.models import EventType, TaskEvent
from gantry.runtime.llm import Message, ToolCallRequest

DEFAULT_SYSTEM_PROMPT = (
    "You are a Gantry worker agent: an autonomous software engineer executing one "
    "well-defined task. Work step by step using the available tools. When the task "
    "is complete, reply with a final message summarizing the outcome instead of "
    "calling more tools."
)


@dataclass
class TrackedMessage:
    """A message plus the event seq it was derived from (None for payload-derived)."""

    seq: int | None
    message: Message


@dataclass
class AgentState:
    tracked: list[TrackedMessage] = field(default_factory=list)
    steps: int = 0  # number of LLM responses so far
    started_tool_ids: set[str] = field(default_factory=set)
    resolved_tool_ids: set[str] = field(default_factory=set)
    prompt_tokens: int = 0
    completion_tokens: int = 0
    resumed: bool = False

    @property
    def messages(self) -> list[Message]:
        return [t.message for t in self.tracked]

    def pending_tool_calls(self) -> list[ToolCallRequest]:
        """Tool calls of the last assistant message that have no result yet."""
        if not self.tracked:
            return []
        last = self.tracked[-1].message
        pending_source = last if last.get("role") == "assistant" else None
        if pending_source is None and len(self.tracked) >= 2:
            # Tool results may already follow the assistant message; walk back.
            for t in reversed(self.tracked):
                if t.message.get("role") == "assistant":
                    pending_source = t.message
                    break
                if t.message.get("role") != "tool":
                    break
        if pending_source is None:
            return []
        pending: list[ToolCallRequest] = []
        for tc in pending_source.get("tool_calls") or []:
            if tc["id"] not in self.resolved_tool_ids:
                pending.append(
                    ToolCallRequest(
                        id=tc["id"],
                        name=tc["function"]["name"],
                        arguments=json.loads(tc["function"]["arguments"]),
                    )
                )
        return pending


def initial_messages(payload: dict[str, Any]) -> list[TrackedMessage]:
    system = payload.get("system_prompt") or DEFAULT_SYSTEM_PROMPT
    goal = payload.get("goal", "")
    return [
        TrackedMessage(None, {"role": "system", "content": system}),
        TrackedMessage(None, {"role": "user", "content": goal}),
    ]


def assistant_message(content: str | None, tool_calls: Sequence[dict[str, Any]]) -> Message:
    """Build an assistant message from llm_response event payload fields."""
    msg: Message = {"role": "assistant", "content": content}
    if tool_calls:
        msg["tool_calls"] = [
            {
                "id": tc["id"],
                "type": "function",
                "function": {"name": tc["name"], "arguments": json.dumps(tc["arguments"])},
            }
            for tc in tool_calls
        ]
    return msg


def tool_message(tool_call_id: str, content: str) -> Message:
    return {"role": "tool", "tool_call_id": tool_call_id, "content": content}


def summary_message(summary: str) -> Message:
    return {
        "role": "user",
        "content": ("[Context compacted — summary of the conversation so far]\n" + summary),
    }


def rehydrate(payload: dict[str, Any], events: Sequence[TaskEvent]) -> AgentState:
    """Rebuild the agent's exact in-flight state from its event log."""
    state = AgentState(tracked=initial_messages(payload))
    for event in events:
        p = event.payload
        if event.event_type is EventType.LLM_RESPONSE:
            state.tracked.append(
                TrackedMessage(
                    event.seq, assistant_message(p.get("content"), p.get("tool_calls") or [])
                )
            )
            state.steps += 1
            usage = p.get("usage") or {}
            state.prompt_tokens += int(usage.get("prompt_tokens", 0))
            state.completion_tokens += int(usage.get("completion_tokens", 0))
            state.resumed = True
        elif event.event_type is EventType.TOOL_CALL:
            state.started_tool_ids.add(p["tool_call_id"])
            state.resumed = True
        elif event.event_type is EventType.TOOL_RESULT:
            state.tracked.append(
                TrackedMessage(event.seq, tool_message(p["tool_call_id"], p["content"]))
            )
            state.resolved_tool_ids.add(p["tool_call_id"])
            state.resumed = True
        elif event.event_type is EventType.COMPACTION:
            kept = set(p["kept_seqs"])
            head = [
                state.tracked[0],  # system message is always payload-derived
                TrackedMessage(event.seq, summary_message(p["summary"])),
            ]
            tail = [t for t in state.tracked if t.seq is not None and t.seq in kept]
            state.tracked = head + tail
            usage = p.get("usage") or {}
            state.prompt_tokens += int(usage.get("prompt_tokens", 0))
            state.completion_tokens += int(usage.get("completion_tokens", 0))
            state.resumed = True
        # llm_request and queue-lifecycle events don't contribute messages.
    return state
