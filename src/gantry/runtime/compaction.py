"""Context compaction: summarize old history into a durable checkpoint.

When the estimated context size crosses the threshold, the loop summarizes
everything except a recent tail (cut at an assistant-message boundary so
tool-call/result pairs are never split), appends a ``compaction`` event
recording the summary and the seqs of the kept messages, and swaps the live
history. Rehydration folds the same event identically — compaction is just
another entry in the log.
"""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass, field

from gantry.runtime.llm import LLMClient, LLMUsage, Message
from gantry.runtime.state import TrackedMessage

TokenCounter = Callable[[list[Message]], int]

SUMMARIZE_INSTRUCTION = (
    "Summarize the conversation above for an agent that will resume this task with "
    "ONLY your summary as context. Include: the original goal, key facts and "
    "decisions, exact file paths / identifiers / commands, work completed so far, "
    "and remaining work. Be precise; do not omit anything needed to continue."
)


def estimate_tokens(messages: list[Message]) -> int:
    """Cheap chars/4 heuristic — deliberately provider-agnostic and dependency-free."""
    total = 0
    for msg in messages:
        total += 8  # per-message structural overhead
        content = msg.get("content")
        if isinstance(content, str):
            total += len(content) // 4
        for tc in msg.get("tool_calls") or []:
            total += len(tc["function"]["arguments"]) // 4 + 16
    return total


@dataclass(frozen=True)
class CompactionConfig:
    max_context_tokens: int = 120_000
    keep_recent_messages: int = 6
    #: Token budget for the verbatim recent tail a size-aware cut keeps. ``None`` ->
    #: ``max_context_tokens // 3``. Bounds the post-compaction floor so compaction
    #: fires every N steps (the implicit cache can re-warm between cuts) instead of
    #: possibly every step.
    keep_recent_tokens: int | None = None
    #: Absolute per-step input ceiling. If a step's projected context still exceeds
    #: this after a normal compaction, an emergency compaction force-shrinks it —
    #: making a 971K-token step impossible by invariant. ``None`` -> 1.5x soft cap.
    hard_max_context_tokens: int | None = None
    token_counter: TokenCounter = field(default=estimate_tokens)

    @property
    def recent_token_budget(self) -> int:
        return (
            self.keep_recent_tokens
            if self.keep_recent_tokens is not None
            else max(1, self.max_context_tokens // 3)
        )

    @property
    def hard_ceiling(self) -> int:
        return (
            self.hard_max_context_tokens
            if self.hard_max_context_tokens is not None
            else int(self.max_context_tokens * 1.5)
        )


@dataclass(frozen=True)
class CompactionPlan:
    cut_index: int  # tracked[cut_index:] is kept; tracked[:cut_index] is summarized


@dataclass(frozen=True)
class CompactionResult:
    summary: str
    kept_seqs: list[int]
    summarized_messages: int
    usage: LLMUsage


def plan_compaction(
    tracked: list[TrackedMessage],
    config: CompactionConfig,
    sized_messages: list[Message] | None = None,
    *,
    force: bool = False,
) -> CompactionPlan | None:
    """Pick a cut point, or None if compaction isn't needed/possible.

    ``sized_messages`` (the PROJECTED, elided messages actually sent) supplies the
    token sizes; it must align 1:1 with ``tracked``. Defaults to the raw messages.

    Size-aware: keep the LARGEST recent tail whose tokens fit ``recent_token_budget``
    (but never fewer than ``keep_recent_messages``, and never split the newest complete
    step), so the post-compaction floor is bounded and compaction fires every N steps.
    The cut lands on an assistant message past the anchors (system 0, goal 1) and the
    first step, so tool-call/result pairs are never split and the anchors stay verbatim.
    ``force`` skips the threshold (emergency path) and targets a minimal tail.
    """
    sized = sized_messages if sized_messages is not None else [t.message for t in tracked]
    if not force and config.token_counter(sized) <= config.max_context_tokens:
        return None
    candidates = [
        i for i, t in enumerate(tracked) if t.message.get("role") == "assistant" and i > 2
    ]
    if not candidates:
        return None
    budget = max(1, config.hard_ceiling // 4) if force else config.recent_token_budget
    floor = 1 if force else config.keep_recent_messages
    tail_tokens = 0
    tail_start = len(tracked)
    for i in range(len(tracked) - 1, 2, -1):
        tail_tokens += config.token_counter([sized[i]])
        if tail_tokens > budget and (len(tracked) - i) >= floor:
            break
        tail_start = i
    at_or_after = [c for c in candidates if c >= tail_start]
    cut = min(at_or_after) if at_or_after else max(candidates)
    return CompactionPlan(cut_index=cut)


async def summarize(
    llm: LLMClient,
    model: str,
    tracked: list[TrackedMessage],
    plan: CompactionPlan,
    head_messages: list[Message] | None = None,
) -> CompactionResult:
    # Summarize the PROJECTED head (elided write bodies) — cheaper and consistent
    # with what the model ever sees; those bodies are being discarded anyway.
    source = head_messages if head_messages is not None else [t.message for t in tracked]
    head = source[: plan.cut_index]
    response = await llm.complete(
        model=model,
        messages=[*head, {"role": "user", "content": SUMMARIZE_INSTRUCTION}],
    )
    kept_seqs = [t.seq for t in tracked[plan.cut_index :] if t.seq is not None]
    return CompactionResult(
        summary=response.content or "",
        kept_seqs=kept_seqs,
        summarized_messages=plan.cut_index,
        usage=response.usage,
    )
