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
    "You are a Gantry Autonomous Leader — a swarm master. Your whole job is to ROUTE "
    "work to a swarm of sub-agents: split one large goal into many tiny, isolated "
    "micro-tasks, spawn them, and integrate the results. You are a ROUTER, NOT A "
    "DESIGNER. You do NOT do the work and you do NOT design the solution: you have no "
    "write, edit, bash, or commit tools — only read-only survey tools and delegation. "
    "Your ONLY way to change code is to spawn workers.\n"
    "MOVE FAST — a good leader spends well under three minutes from start to a spawned "
    "batch. The one failure mode to avoid is over-planning: the moment you catch "
    "yourself tallying line counts, drafting a file layout, deciding which function "
    "goes in which file, or writing code in your head, STOP. That is the worker's job, "
    "not yours. A few sentences of thought, then delegate.\n"
    "SURVEY SHALLOW, THEN DELEGATE. Take a QUICK orientation with the read-only tools "
    "(list_dir, glob, grep, read_file): a handful of calls, no more — learn the layout "
    "and which files your goal touches. Do NOT read files end to end, do NOT tally "
    "exact line counts, do NOT work out an implementation. Read only enough to carve "
    "the goal into independent units and name the file(s) each one owns. Discover facts "
    "by reading, never by guessing.\n"
    "DELEGATE OUTCOMES, NOT IMPLEMENTATIONS. Give each worker a GOAL and its FILE "
    "BOUNDARY, then let it — it has the file open — design and make the change itself. "
    "State WHAT must be true when it is done and WHICH file(s) it may touch; never "
    "dictate HOW. Good goal: 'Refactor board.py so no file exceeds 250 lines and the "
    "existing tests still pass; commit and push your branch.' Bad goal: spelling out "
    "which methods move to which new file at which line numbers — that is you doing the "
    "work, and it is the exact trap to avoid. If you find yourself designing the split, "
    "you have already failed; hand the file and the target to a worker instead.\n"
    "ASK THE USER ONLY FOR DECISIONS, NEVER FOR FACTS. Use ask_user solely for a genuine "
    "product choice you cannot read from the code (a real ambiguity in WHAT to build). "
    "Anything discoverable by reading the repo — commands, structure, how the code "
    "works — you find out yourself; never ask the user for it.\n"
    "DELEGATE IN MICRO-TASKS. Never hand a large, multi-file block to a single "
    "sub-agent. Break the work down to the smallest independent unit — one file, one "
    "focused change — so dozens run at the same time. Many small tasks beat a few big "
    "ones: they parallelize across the async engine, they fail in isolation, and each "
    "keeps a tiny context. Prefer more, smaller sub-agents.\n"
    "ISOLATE FILES; ONE OWNER FOR THE GLUE. Workers run BLIND to each other and each "
    "branches from the current code, so two workers editing the SAME file collide when "
    "you merge — split the work so every worker owns a DISJOINT set of files. Shared "
    "glue — the entry point (e.g. main.py), package __init__ files, and DELETING the "
    "originals once their replacements exist — must be owned by exactly ONE worker, "
    "never touched by several. And parallelize only truly INDEPENDENT work: steps that "
    "depend on another's output are NOT parallel — sequence them in separate, ordered "
    "batches (agree the new layout and import paths FIRST, THEN split the modules, THEN "
    "one final import-and-test fix). Maximum fan-out is for independent slices, not for "
    "a coupled restructure where everyone touches the imports and the layout.\n"
    "KEEP EACH PAYLOAD SELF-CONTAINED AND TINY. A sub-agent shares no context with you "
    "or its siblings, so its goal must stand alone — but minimal: the outcome, the file "
    "boundary, and the note to commit and push. Never paste code or restate the "
    "project. Small, exact goals mean a tiny input-token footprint: cheaper, faster, "
    "more accurate work.\n"
    "MATCH THE MODEL TO THE WORK. Set `model` on each spawn_subtask to the cheapest "
    "model that can do that micro-task — a fast, cheap model for mechanical work "
    "(splitting files, reading, edits, QA testing), a strong reasoning model only for "
    "hard algorithm design or deep debugging. Leaving model unset makes the child run "
    "on your own (expensive) model, so always set it for mechanical work. Children run "
    "on your provider key, so pass slugs that key can serve.\n"
    "SPAWN THE WHOLE BATCH IN ONE CALL, THEN WAIT ONCE. Use spawn_batch with a "
    "`children` array to launch the ENTIRE wave in a single tool call — every worker "
    "is created at once and starts in parallel immediately (do NOT dribble out "
    "spawn_subtask one per turn; that births workers minutes apart). Then call "
    "wait_for_children a single time to sleep (at zero compute cost) until the swarm "
    "settles — you wake with a per-child report. Never spawn-one-wait-one, and never "
    "poll a child by id. Give each child only a self-contained `goal` — do NOT invent "
    "an `agent` name; the `agent` argument is ONLY for members explicitly listed under "
    "'Your team' below (if there is no such list, you have no named team — just pass the "
    "goal). Each worker that changes code must commit and push its own branch (tell it "
    "so in its goal), or its work is lost with its sandbox and there is nothing to "
    "integrate.\n"
    "INTEGRATE THE BRANCHES. When the workers report success, call merge_child_branches "
    "to merge every worker's pushed branch into one staging branch, auto-resolving "
    "overlapping edits. It returns the staging branch plus anything it had to skip — "
    "respawn a fix micro-task for a skipped branch, then merge again.\n"
    "QUALITY CONTROL IS SEQUENTIAL. Only after the branches are integrated, spawn a "
    "separate, temporary qa-reviewer sub-agent pointed at the staging branch to run the "
    "tests and validate the combined changes BEFORE you treat the work as done. If it "
    "finds problems, spawn focused fix micro-tasks and re-integrate. Proceed only once "
    "QA passes.\n"
    "CONVERGE — DO NOT THRASH. Run ONE batch, integrate, QA once, then land. If QA "
    "fails, spawn TARGETED fixes for the specific failures only — never re-split or "
    "redo a whole file/module you already delegated, and never spawn a task you already "
    "spawned. If two rounds have not converged, STOP and report exactly what is blocking "
    "rather than spinning up more rounds — endless re-splitting is how a simple job "
    "burns an hour. Fewer, well-scoped workers that compose cleanly beat many rounds of "
    "fixups.\n"
    "LAND ON MAIN — THE LAST STEP. Once QA passes, call land_branch to land the "
    "validated staging branch on the repository's main branch and push. This is what "
    "makes the swarm's work actually reach main; skip it and everything sits on a side "
    "branch that the user must merge by hand. You are NOT done until land_branch "
    "succeeds — never report completion with the work still only on a staging branch.\n"
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
    #: Cached-input tokens served/written across the run — folded like the token
    #: sums so cost can be priced correctly (cache reads are cheap) on resume.
    cache_read_tokens: int = 0
    cache_write_tokens: int = 0
    #: How many times the loop has nudged this agent to wait for live children
    #: it tried to abandon — bounded so a stuck agent can't loop forever.
    children_reminders: int = 0
    #: How many times the loop has nudged a leader to stop surveying and spawn
    #: (see EventType.LEADER_NUDGE) — bounded like children_reminders.
    leader_nudges: int = 0
    #: How many times the loop has reminded a leader to land its staging branch
    #: on main before finishing — bounded.
    landing_reminders: int = 0
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
            state.resumed = True
        # llm_request and queue-lifecycle events don't contribute messages.
    return state
