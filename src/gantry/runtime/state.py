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
    #: Cached-input tokens served/written across the run — folded like the token
    #: sums so cost can be priced correctly (cache reads are cheap) on resume.
    cache_read_tokens: int = 0
    cache_write_tokens: int = 0
    #: How many times this run compacted its history (folded from COMPACTION
    #: events). An observability signal: frequent compaction means the prefix cache
    #: keeps resetting, so per-step input is climbing.
    compactions: int = 0
    #: How many times the loop has nudged this agent to wait for live children
    #: it tried to abandon — bounded so a stuck agent can't loop forever.
    children_reminders: int = 0
    #: How many times the loop has nudged a leader to stop surveying and spawn
    #: (see EventType.LEADER_NUDGE) — bounded like children_reminders.
    leader_nudges: int = 0
    #: How many times the loop has reminded a leader to land its staging branch
    #: on main before finishing — bounded.
    landing_reminders: int = 0
    #: Error fingerprints per diagnostic-producing tool result, oldest first —
    #: the loop detector's rolling evidence. Folded from DIAGNOSTICS events so a
    #: resumed task keeps counting where it left off instead of forgetting that
    #: it is stuck (which is how a "stuck" task used to survive its own retries).
    diagnostic_batches: list[list[str]] = field(default_factory=list)
    #: Human-readable renderings of the same errors ("src/x.rs:4:20: error[E0308]:
    #: ..."), so an escalation can show WHAT kept failing rather than a hash.
    diagnostic_lines: list[str] = field(default_factory=list)
    #: Model this task was escalated to after stalling, or None. Overrides the
    #: payload's model, so the escalation is durable and survives a re-claim.
    escalated_model: str | None = None
    #: How many times this task has been escalated — bounded so a task that
    #: stalls again under the stronger model fails loudly instead of ping-ponging.
    escalations: int = 0
    resumed: bool = False

    @property
    def messages(self) -> list[Message]:
        return [t.message for t in self.tracked]

    def projected_messages(self) -> list[Message]:
        """The message list actually SENT to the model: raw history with
        already-resolved write_file/edit_file arguments elided (see
        ``elide_resolved_writes``). A pure, deterministic projection — it never
        mutates ``self.tracked`` or the event log, so live and rehydrated states
        project byte-identically, and a pending/unresolved write keeps its real
        args for ``recover()``/re-execution (which read from ``tracked``)."""
        return [elide_resolved_writes(t.message, self.resolved_tool_ids) for t in self.tracked]

    def count_tool_calls(self, *names: str) -> int:
        """How many times the agent has called any of ``names`` so far, counted
        from the folded message history (so it is identical after a resume)."""
        wanted = set(names)
        return sum(
            1
            for t in self.tracked
            if t.message.get("role") == "assistant"
            for tc in (t.message.get("tool_calls") or [])
            if tc["function"]["name"] in wanted
        )

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


#: Tools whose (large) file-body arguments needn't recirculate in context once
#: the write has executed — the file is on disk; the agent re-reads if it needs it.
_ELIDABLE_TOOLS = frozenset({"write_file", "edit_file"})
#: The bulky argument fields those tools carry.
_ELIDABLE_ARG_KEYS = ("content", "new_str", "old_str")


def elide_resolved_writes(message: Message, resolved: set[str]) -> Message:
    """Return the message to SEND, with an already-resolved write_file/edit_file
    tool-call's bulky file-body args replaced by a compact ``[gantry: …]`` note.

    This is a pure prompt-assembly projection — it copies, never mutating the
    input — so it is deterministic given ``(message, resolved)`` and byte-identical
    live vs. rehydrated. Only tool-calls in ``resolved`` are elided: a pending or
    crash-interrupted write keeps real args (``recover()``/re-execution read them
    from the untouched history). The placeholder is an unambiguous system
    annotation, never plausible as the file's contents, and it tells the agent to
    re-read the file if it needs the bytes."""
    if message.get("role") != "assistant":
        return message
    calls = message.get("tool_calls")
    if not calls:
        return message
    new_calls: list[dict[str, Any]] | None = None
    for idx, tc in enumerate(calls):
        fn = tc.get("function") or {}
        if fn.get("name") not in _ELIDABLE_TOOLS or tc.get("id") not in resolved:
            continue
        try:
            args = json.loads(fn.get("arguments") or "{}")
        except ValueError:
            continue
        if not isinstance(args, dict):
            continue
        written = sum(len(args[k]) for k in _ELIDABLE_ARG_KEYS if isinstance(args.get(k), str))
        if written == 0:
            continue
        path = args.get("path", "?")
        kept = {k: v for k, v in args.items() if k not in _ELIDABLE_ARG_KEYS}
        kept["_gantry_elided"] = (
            f"[gantry: file written — {written} chars to {path}; "
            "re-read the file if you need its contents]"
        )
        if new_calls is None:
            new_calls = [dict(c) for c in calls]
        new_calls[idx] = {**tc, "function": {**fn, "arguments": json.dumps(kept)}}
    if new_calls is None:
        return message
    return {**message, "tool_calls": new_calls}


def compaction_anchors(tracked: list[TrackedMessage]) -> list[TrackedMessage]:
    """The head every compaction keeps VERBATIM: the system prompt (index 0) and
    the root goal/spec (index 1). Both are payload-derived (``seq=None``) and must
    survive every fold — normal, size-aware, or emergency — so a long run never
    summarizes away its own instructions or specification. Used identically by the
    live fold and by rehydration, so a resume reconstructs byte-identical history."""
    return tracked[:2]


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


#: The read-only tools a leader surveys the repo with. Counting calls to these
#: (against SPAWN_TOOL_NAMES) is how the loop detects a leader that keeps
#: reading instead of delegating.
SURVEY_TOOL_NAMES = frozenset({"read_file", "list_dir", "glob", "grep"})
#: The delegation tools whose first use means the leader has started routing
#: work — once one fires, the survey-budget nudge stops.
SPAWN_TOOL_NAMES = frozenset({"spawn_subtask", "spawn_batch"})

#: Read-only tools: they observe the repo but change nothing. A leaf worker that
#: makes many of these and NONE of the productive tools below is passively looping
#: (reading forever) rather than doing the work it was handed.
READ_ONLY_TOOL_NAMES = frozenset(
    {"read_file", "list_dir", "glob", "grep", "web_search", "web_fetch"}
)
#: Tools that make real progress — mutate the workspace, run a command, or
#: delegate. One call to any of these means the worker is not merely reading, so
#: the passive-read breaker stays silent.
PRODUCTIVE_TOOL_NAMES = frozenset(
    {
        "write_file",
        "edit_file",
        "git_commit_push",
        "bash",
        "spawn_subtask",
        "spawn_batch",
        "merge_child_branches",
        "land_branch",
    }
)


def leader_nudge_message(surveyed: int) -> Message:
    """The reminder injected when an autonomous leader has surveyed past its
    budget without spawning anyone — the over-planning failure mode. Pushes it
    to stop reading and dispatch the batch now."""
    return {
        "role": "user",
        "content": (
            f"You have now made {surveyed} read-only survey calls and spawned zero "
            "workers. That is enough looking. STOP surveying — do not read another "
            "file, tally line counts, or design the solution. Right now, split the "
            "goal into independent micro-tasks and call spawn_subtask for each one "
            "(a whole batch), giving every worker a file boundary and an outcome and "
            "letting it design the change itself. Then call wait_for_children once. "
            "Your job is to hand out work, not to study it."
        ),
    }


def leader_land_message() -> Message:
    """The reminder injected when a leader tries to finish after integrating the
    workers' branches but without landing them on main — so the result would sit
    on a staging branch a human then has to merge by hand."""
    return {
        "role": "user",
        "content": (
            "You are not done: you merged the workers' branches into a staging branch "
            "but never landed it on main. The user's work is stuck on a side branch. "
            "If QA validated the integrated result, call land_branch NOW to land the "
            "staging branch on main and push. If you are deliberately abandoning this "
            "work, say so explicitly instead."
        ),
    }


#: How many diagnostic-producing results back the detector looks. A plain
#: three-in-a-row repeat is the case in the spec; the window also catches the
#: alternating form ("fix A, break B, fix B, break A"), which is the same
#: stagnation wearing a different shape and would otherwise never show three
#: CONSECUTIVE identical results.
LOOP_WINDOW = 6
#: Occurrences of one fingerprint within that window that mean "stuck".
LOOP_THRESHOLD = 3


def _rendered(payload: Sequence[Any]) -> list[str]:
    """Render DIAGNOSTICS event entries back to their one-line form.

    Only ERRORs: warnings are not what the detector acts on, so showing them in
    an escalation would dilute the evidence.
    """
    lines: list[str] = []
    for entry in payload:
        if not isinstance(entry, dict) or entry.get("severity") != "error":
            continue
        where = ":".join(
            str(part) for part in (entry.get("file"), entry.get("line")) if part is not None
        )
        code = f" [{entry['code']}]" if entry.get("code") else ""
        lines.append(f"{where}{code}: {entry.get('message', '')}")
    return lines


def repeated_error(
    batches: list[list[str]],
    *,
    window: int = LOOP_WINDOW,
    threshold: int = LOOP_THRESHOLD,
) -> str | None:
    """The fingerprint of an error that keeps coming back, or None.

    Counts DISTINCT results containing each fingerprint, not raw occurrences: a
    single compile that reports the same error five times across targets is one
    failure, and must not look like five attempts at it.
    """
    if len(batches) < threshold:
        return None
    counts: dict[str, int] = {}
    for batch in batches[-window:]:
        for fingerprint in set(batch):
            counts[fingerprint] = counts.get(fingerprint, 0) + 1
    stuck = [f for f, n in counts.items() if n >= threshold]
    if not stuck:
        return None
    # Deterministic pick, so a resumed run escalates on the identical error.
    return min(stuck)


def escalation_message(model: str | None, history: Sequence[str]) -> Message:
    """The context handed to the escalated (stronger) run.

    Deliberately blunt about the failure mode. An agent that has been looping has
    a context full of near-identical attempts, all of which look reasonable; a
    polite note gets pattern-matched into "try again". This states the evidence
    (the same error, N times), forbids the move that failed, and asks for a
    different approach — with the structured error list rather than the logs it
    already failed to learn from.
    """
    listed = "\n".join(f"  - {line}" for line in history)
    switched = (
        f"You are a stronger model ({model}) brought in to break the deadlock. " if model else ""
    )
    return {
        "role": "user",
        "content": (
            "STOP. You are in a repair loop: the same error has now been produced "
            f"repeatedly by your last several attempts.\n{listed}\n\n"
            f"{switched}Repeating the previous edit — or any small variation of it — "
            "will fail the same way. Do not try it again. Instead: re-read the "
            "relevant code and the error above, work out WHY the fix keeps failing "
            "(a wrong assumption about a type, an API, or a file's actual contents "
            "is the usual cause), and take a different approach. If the goal cannot "
            "be met as specified, say so explicitly in your final message and "
            "explain what blocks it — that is a useful result; another identical "
            "attempt is not."
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
            state.cache_read_tokens += int(usage.get("cache_read_tokens", 0))
            state.cache_write_tokens += int(usage.get("cache_write_tokens", 0))
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
        elif event.event_type is EventType.LEADER_NUDGE:
            if p.get("land"):
                state.tracked.append(TrackedMessage(event.seq, leader_land_message()))
                state.landing_reminders += 1
            else:
                state.tracked.append(
                    TrackedMessage(event.seq, leader_nudge_message(int(p.get("surveyed", 0))))
                )
                state.leader_nudges += 1
            state.resumed = True
        elif event.event_type is EventType.DIAGNOSTICS:
            # Evidence only — contributes no message. The agent already saw the
            # errors in the tool result; re-injecting them would double the very
            # tokens this layer exists to save.
            state.diagnostic_batches.append([str(f) for f in p.get("fingerprints") or []])
            state.diagnostic_lines.extend(_rendered(p.get("diagnostics") or []))
            state.resumed = True
        elif event.event_type is EventType.TASK_ESCALATED:
            state.escalations += 1
            state.escalated_model = p.get("model") or state.escalated_model
            state.tracked.append(
                TrackedMessage(
                    event.seq, escalation_message(p.get("model"), p.get("history") or [])
                )
            )
            # The escalated run starts a fresh evidence window: the point is to
            # judge the NEW approach on its own, not to re-fire the breaker on
            # the history that triggered it.
            state.diagnostic_batches.clear()
            state.diagnostic_lines.clear()
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
            # System (0) and goal (1) are untouchable anchors — kept verbatim so a
            # long run never summarizes away its own instructions/spec. Must mirror
            # the live fold in loop.py exactly, or a resume diverges.
            head = [
                *compaction_anchors(state.tracked),
                TrackedMessage(event.seq, summary_message(p["summary"])),
            ]
            tail = [t for t in state.tracked if t.seq is not None and t.seq in kept]
            state.tracked = head + tail
            usage = p.get("usage") or {}
            state.prompt_tokens += int(usage.get("prompt_tokens", 0))
            state.completion_tokens += int(usage.get("completion_tokens", 0))
            state.cache_read_tokens += int(usage.get("cache_read_tokens", 0))
            state.cache_write_tokens += int(usage.get("cache_write_tokens", 0))
            state.compactions += 1  # observability counter; folded live in loop too
            state.resumed = True
        # llm_request and queue-lifecycle events don't contribute messages.
    return state
