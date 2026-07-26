"""The scenario state machine.

A ``Scenario`` is a pure function ``(step, messages) -> Turn``. The step index is
derived *statelessly* from the request itself (the count of prior assistant turns
in ``messages``), so the mock needs no server-side session store: it is naturally
idempotent under retries and safe under concurrency, and it satisfies the
"hash the conversation history" requirement without extra state.

A ``Turn`` is the model's next response. Exactly one thing ends it:

- ``tool_calls`` non-empty  -> ``finish_reason="tool_calls"``; Gantry runs the
  tools (for real, in its sandbox) and calls back for the next step.
- ``tool_calls`` empty      -> ``finish_reason="stop"``; ``content`` is the final
  answer and the agent loop completes.

``content`` may ALSO accompany tool calls (the token wall the runaway scenario
streams before ending each turn on a harmless tool call, so the loop never
finishes and a breaker has to stop it).
"""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass
from typing import Any

#: OpenAI-style chat message dict.
Message = dict[str, Any]


@dataclass(frozen=True)
class ToolCall:
    name: str
    arguments: dict[str, Any]
    id: str = "call_mock"


@dataclass(frozen=True)
class Turn:
    #: Assistant text. For the runaway it is streamed as ``content_chunks`` frames.
    content: str = ""
    tool_calls: tuple[ToolCall, ...] = ()
    prompt_tokens: int = 64
    completion_tokens: int = 16
    #: Number of SSE frames to split ``content`` across (the runaway's token wall).
    content_chunks: int = 1
    #: Emit a mid-stream ``usage`` frame every N content frames (0 = only a single
    #: trailing usage frame). Lets a run watch spend accrue before the turn ends.
    usage_every: int = 0

    @property
    def finish_reason(self) -> str:
        return "tool_calls" if self.tool_calls else "stop"


#: A scenario picks the next Turn from the derived step and the raw history.
Scenario = Callable[[int, list[Message]], Turn]


def step_of(messages: list[Message]) -> int:
    """The current step: how many assistant turns already happened. Stateless, so
    a resumed/retried request lands on the same step as the original."""
    return sum(1 for m in messages if m.get("role") == "assistant")


# --- happy-path ---------------------------------------------------------------
# Read a file, write a (valid) file, then finish — succeeds in two tool steps.
HAPPY_READ_PATH = "input.txt"
HAPPY_WRITE_PATH = "output.py"
HAPPY_WRITE_BODY = "RESULT = 42\n"


def happy_path(step: int, messages: list[Message]) -> Turn:
    if step == 0:
        return Turn(tool_calls=(ToolCall("read_file", {"path": HAPPY_READ_PATH}),))
    if step == 1:
        return Turn(
            tool_calls=(
                ToolCall("write_file", {"path": HAPPY_WRITE_PATH, "content": HAPPY_WRITE_BODY}),
            )
        )
    return Turn(content="Done: read the input and wrote output.py in two steps.")


# --- repair-loop --------------------------------------------------------------
# Rewrite the SAME syntactically-broken file every step. Gantry's real write-time
# AST check emits an identical-fingerprint diagnostic each time; after three
# recurrences the repair-wave breaker stalls/escalates the task.
REPAIR_PATH = "broken.py"
REPAIR_BODY = "def unfinished(\n"  # a genuine SyntaxError, byte-for-byte stable


def repair_loop(step: int, messages: list[Message]) -> Turn:
    return Turn(tool_calls=(ToolCall("write_file", {"path": REPAIR_PATH, "content": REPAIR_BODY}),))


# --- budget-runaway -----------------------------------------------------------
# Stream a wall of tokens with periodic usage, then end on a harmless tool call so
# the loop keeps going and NEVER produces a final answer. Stands in for an
# unbounded stream; a real run is stopped by the finite step cap today (and, once
# the live metering sentinel lands, by a mid-stream connection cut).
RUNAWAY_CHUNKS = 200
_RUNAWAY_PIECE = "tokens tokens tokens tokens "


def budget_runaway(step: int, messages: list[Message]) -> Turn:
    return Turn(
        content=_RUNAWAY_PIECE,
        tool_calls=(ToolCall("list_dir", {"path": "."}),),
        prompt_tokens=64,
        completion_tokens=RUNAWAY_CHUNKS,
        content_chunks=RUNAWAY_CHUNKS,
        usage_every=25,
    )


# --- hallucinated-tool --------------------------------------------------------
# Call a tool that does not exist, every step. The loop returns a structured
# UnknownTool diagnostic (same fingerprint each time), so the repair-wave breaker
# escalates instead of letting the agent burn its budget on a phantom tool.
HALLUCINATED_TOOL_NAME = "frobnicate"


def hallucinated_tool(step: int, messages: list[Message]) -> Turn:
    return Turn(tool_calls=(ToolCall(HALLUCINATED_TOOL_NAME, {"target": "everything"}),))


# --- passive-read-loop --------------------------------------------------------
# Only ever read (never edit/write/commit). A leaf worker doing this is stuck
# circling the problem; the passive-read breaker fails it fast so its leader learns.
def passive_read_loop(step: int, messages: list[Message]) -> Turn:
    return Turn(tool_calls=(ToolCall("list_dir", {"path": "."}),))


# --- git-conflict (conflict RESOLVER) -----------------------------------------
# Used as Gantry's LLM conflict resolver: when two workers edit colliding lines,
# merge_child_branches hands the conflicted file (with markers) to this model and
# expects the reconciled content back. The mock union-merges deterministically.
def _resolve_conflict_markers(text: str) -> str:
    """Drop the ``<<<<<<<`` / ``=======`` / ``>>>>>>>`` lines, keeping BOTH sides —
    a naive but real resolution the mock can compute without an LLM."""
    kept = [
        line for line in text.splitlines() if not line.startswith(("<<<<<<<", "=======", ">>>>>>>"))
    ]
    return "\n".join(kept) + ("\n" if text.endswith("\n") else "")


def git_conflict(step: int, messages: list[Message]) -> Turn:
    # The resolver prompt is "File: <path>\n\n<conflicted content>"; return the
    # reconciled file (markers removed, both edits kept).
    raw = str(messages[-1].get("content", "")) if messages else ""
    body = raw.split("\n\n", 1)[1] if "\n\n" in raw else raw
    return Turn(content=_resolve_conflict_markers(body))


# --- missing-branch -----------------------------------------------------------
# A worker "claiming success". Delivery to origin is now guaranteed by
# ensure_pushed at finalize, so a missing branch can no longer be manufactured by
# the LLM — the MissingBranch diagnostic is exercised at the merge layer (see the
# merge-layer chaos test). This turn documents the scenario in the switcher.
def missing_branch(step: int, messages: list[Message]) -> Turn:
    return Turn(content="Work complete.")


# --- transport-level chaos (handled in app.py, not as token scenarios) --------
#: Return HTTP 429 for this many requests, then a clean success. The provider
#: client (LiteLLM) retries transient 429s with its own backoff, so the run
#: recovers; Gantry's queue backoff is the second line if the client exhausts.
RATE_LIMIT_MODEL = "mock/rate-limit-429"
RATE_LIMIT_FAILURES = 2
#: Emit a malformed SSE chunk this many times, then a clean stream — the client
#: raises on the bad chunk and the run recovers on retry, never a hard failure.
CORRUPTED_SSE_MODEL = "mock/corrupted-sse"
CORRUPTED_SSE_FAILURES = 2
#: Models whose chaos is at the HTTP/stream layer (status codes, aborted bodies)
#: rather than in the emitted tokens — app.py special-cases these.
TRANSPORT_CHAOS_MODELS = frozenset({RATE_LIMIT_MODEL, CORRUPTED_SSE_MODEL})


#: Registry keyed by the raw ``model`` string in the request body.
SCENARIOS: dict[str, Scenario] = {
    "mock/happy-path": happy_path,
    "mock/repair-loop": repair_loop,
    "mock/budget-runaway": budget_runaway,
    "mock/hallucinated-tool": hallucinated_tool,
    "mock/passive-read-loop": passive_read_loop,
    "mock/git-conflict": git_conflict,
    "mock/missing-branch": missing_branch,
}

#: Prefixes LiteLLM may leave on the model when routing an OpenAI-compatible base.
_STRIPPABLE_PREFIXES = ("openai/",)


def select(model: str) -> Scenario:
    """The scenario for ``model``. Tolerant of a leftover ``openai/`` route prefix;
    an unknown model falls back to happy-path so a stray call never 500s."""
    if model in SCENARIOS:
        return SCENARIOS[model]
    for prefix in _STRIPPABLE_PREFIXES:
        if model.startswith(prefix) and model[len(prefix) :] in SCENARIOS:
            return SCENARIOS[model[len(prefix) :]]
    return happy_path
