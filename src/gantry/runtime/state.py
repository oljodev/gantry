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
    "calling more tools. Keep that final summary tight and factual: it is a handoff "
    "the agent that spawned you reads back into its own context, so state what "
    "changed and where (files, identifiers, results) — not a narration of every step."
)

AUTONOMOUS_LEADER_PROMPT = (
    "You are a Gantry Autonomous Leader — a swarm master. You decompose one large "
    "goal into many tiny, isolated micro-tasks and run them as a concurrent swarm of "
    "sub-agents. You do NOT write code yourself: you plan, delegate, and integrate.\n"
    "DELEGATE IN MICRO-TASKS. Never hand a large, multi-file block to a single "
    "sub-agent. Break the work down to the smallest independent unit — one function, "
    "one file, one focused change — so dozens run at the same time. Many small tasks "
    "beat a few big ones: they parallelize across the async engine, they fail in "
    "isolation, and each keeps a tiny context. Prefer more, smaller sub-agents.\n"
    "KEEP EACH PAYLOAD TINY. A sub-agent shares no context with you or its siblings, "
    "so its goal must be fully self-contained — but minimal. Give it only the precise "
    "instruction and the exact file/function/lines it must touch; never paste "
    "unrelated code or restate the whole project. Small, exact instructions mean a "
    "tiny input-token footprint: cheaper, faster, and more accurate work.\n"
    "SPAWN THE WHOLE BATCH, THEN WAIT ONCE. Call spawn_subtask once per micro-task, "
    "launching the entire batch up front so they execute in parallel, then call "
    "wait_for_children a single time to sleep (at zero compute cost) until the swarm "
    "settles — you wake with a per-child report. Never spawn-one-wait-one. Give each "
    "spawn_subtask only a self-contained `goal` — do NOT invent an `agent` name; the "
    "`agent` argument is ONLY for members explicitly listed under 'Your team' below (if "
    "there is no such list, you have no named team — just pass the goal). Each worker "
    "that changes code must commit and push its own branch (tell it so in its goal), or "
    "its work is lost with its sandbox and there is nothing to integrate.\n"
    "INTEGRATE THE BRANCHES. When the workers report success, call merge_child_branches "
    "to merge every worker's pushed branch into one staging branch, auto-resolving "
    "overlapping edits. It returns the staging branch plus anything it had to skip — "
    "respawn a fix micro-task for a skipped branch, then merge again.\n"
    "QUALITY CONTROL IS SEQUENTIAL. Only after the branches are integrated, spawn a "
    "separate, temporary qa-reviewer sub-agent pointed at the staging branch to run the "
    "tests and validate the combined changes BEFORE you treat the work as done. If it "
    "finds problems, spawn focused fix micro-tasks and re-integrate. Finish only once "
    "QA passes.\n"
    "If a child failed, decide: respawn it with a refined goal, work around it, or "
    "abort with an explanation. Pass repo_url only to work on an EXISTING repo; for a "
    "new project omit it so the child gets an empty workspace to `git init` — never "
    "invent a placeholder repo URL.\n"
    "When the goal is achieved, reply with one tight, factual message that integrates "
    "the swarm's results instead of calling more tools — a handoff, not a narration."
)

#: The leader/orchestrator prompt. Kept under the historical name so existing
#: imports (API launch, spawn_subtask, team planner nodes) resolve unchanged.
PLANNER_SYSTEM_PROMPT = AUTONOMOUS_LEADER_PROMPT


@dataclass
class TrackedMessage:
    """A message plus the event seq it was derived from (None for payload-derived)."""

    seq: int | None
    message: Message


@dataclass(frozen=True)
class ApprovalState:
    """Where a gated tool call stands: requested, or resolved by a human."""

    decision: str  # "requested" | "approved" | "rejected"
    comment: str = ""


@dataclass
class AgentState:
    tracked: list[TrackedMessage] = field(default_factory=list)
    steps: int = 0  # number of LLM responses so far
    started_tool_ids: set[str] = field(default_factory=set)
    resolved_tool_ids: set[str] = field(default_factory=set)
    #: Approval status per gated tool_call_id (from approval_* events).
    approvals: dict[str, ApprovalState] = field(default_factory=dict)
    #: Skills already injected (from skill_injected events) — injection is
    #: idempotent per skill name, so a crash mid-injection resumes cleanly.
    injected_skills: set[str] = field(default_factory=set)
    #: Gated calls whose post-approval execution has begun (tool_started).
    gated_started_ids: set[str] = field(default_factory=set)
    prompt_tokens: int = 0
    completion_tokens: int = 0
    #: How many times the loop has nudged this agent to wait for live children
    #: it tried to abandon — bounded so a stuck agent can't loop forever.
    children_reminders: int = 0
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


def children_pending_message(children: Sequence[str]) -> Message:
    """The reminder injected when a spawning agent tries to finish while children
    it launched are still running — steer it back to wait_for_children rather
    than let it report success and orphan their work."""
    listed = ", ".join(children) if children else "some you spawned"
    return {
        "role": "user",
        "content": (
            f"You are not done: child agents you spawned are still running ({listed}). "
            "Do NOT report completion while they work — their results would be lost. "
            "Call wait_for_children to sleep until they all finish, then integrate what "
            "they produced (and commit/push if that is your responsibility) before you "
            "reply with a final message."
        ),
    }


def apply_skill_to_system_message(state: AgentState, name: str, content: str) -> None:
    """Append one skill's instructions to the system message, exactly once.

    Used by both the live injection path and rehydration, so a resumed run
    reconstructs a byte-identical system prompt.
    """
    if name in state.injected_skills:
        return
    system = state.tracked[0].message
    system["content"] = f"{system.get('content') or ''}\n\n## Skill: {name}\n\n{content}"
    state.injected_skills.add(name)


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
        elif event.event_type is EventType.TOOL_STARTED:
            state.gated_started_ids.add(p["tool_call_id"])
            state.resumed = True
        elif event.event_type is EventType.APPROVAL_REQUESTED:
            state.approvals[p["tool_call_id"]] = ApprovalState("requested")
            state.resumed = True
        elif event.event_type is EventType.SKILL_INJECTED:
            apply_skill_to_system_message(state, str(p["name"]), str(p["content"]))
            state.resumed = True
        elif event.event_type is EventType.CHILDREN_PENDING:
            state.tracked.append(
                TrackedMessage(event.seq, children_pending_message(p.get("children") or []))
            )
            state.children_reminders += 1
            state.resumed = True
        elif event.event_type is EventType.APPROVAL_RESOLVED:
            state.approvals[p["tool_call_id"]] = ApprovalState(
                str(p.get("decision")), str(p.get("comment") or "")
            )
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
