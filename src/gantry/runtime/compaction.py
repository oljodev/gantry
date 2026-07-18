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
    token_counter: TokenCounter = field(default=estimate_tokens)


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
    tracked: list[TrackedMessage], config: CompactionConfig
) -> CompactionPlan | None:
    """Pick a cut point, or None if compaction isn't needed/possible.

    The cut must land on an assistant message (so the kept tail starts a
    complete step) and must leave something substantial to summarize.
    """
    if config.token_counter([t.message for t in tracked]) <= config.max_context_tokens:
        return None
    target_start = len(tracked) - config.keep_recent_messages
    candidates = [
        i for i, t in enumerate(tracked) if t.message.get("role") == "assistant" and i > 2
    ]
    if not candidates:
        return None
    at_or_after = [i for i in candidates if i >= target_start]
    cut = min(at_or_after) if at_or_after else max(candidates)
    return CompactionPlan(cut_index=cut)


async def summarize(
    llm: LLMClient,
    model: str,
    tracked: list[TrackedMessage],
    plan: CompactionPlan,
) -> CompactionResult:
    head = [t.message for t in tracked[: plan.cut_index]]
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
