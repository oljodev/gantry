"""Model pricing for the run cost ledger / budget brake — pure, no network."""

from __future__ import annotations

from gantry.runtime.llm import LLMUsage
from gantry.runtime.pricing import _FALLBACK, cost_usd, price_for


def test_cost_prices_input_and_output_per_mtoken() -> None:
    # opus: input $5, output $25 per 1M tokens.
    usage = LLMUsage(prompt_tokens=1_000_000, completion_tokens=1_000_000)
    assert cost_usd(usage, "anthropic/claude-opus-4-8") == 30.0


def test_cache_reads_are_billed_at_the_cheap_rate() -> None:
    # All input served from cache -> cheap read rate ($0.5/Mtok), not input ($5).
    cached = LLMUsage(prompt_tokens=1_000_000, completion_tokens=0, cache_read_tokens=1_000_000)
    assert cost_usd(cached, "anthropic/claude-opus-4-8") == 0.5


def test_unknown_model_uses_the_conservative_fallback() -> None:
    assert price_for("some/unheard-of-model") is _FALLBACK


def test_matches_by_bare_slug_when_prefix_differs() -> None:
    # A bare 'qwen/qwen3-coder' still matches the openrouter/... table entry.
    assert price_for("qwen/qwen3-coder").input_per_mtok == 1.0
