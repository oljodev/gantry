"""Model pricing → USD, for the per-run cost ledger and budget brake.

Deliberately a small, stable table plus a conservative fallback: the exact
figure matters less than a deterministic, monotonic estimate the $ brake can
trust across resumes. Cached-input tokens are priced separately (cheap) so the
brake doesn't trip early once prompt caching engages. Prices are USD per 1M
tokens; override the table for your provider as needed.
"""

from __future__ import annotations

from dataclasses import dataclass

from gantry.runtime.llm import LLMUsage

_M = 1_000_000.0


@dataclass(frozen=True)
class ModelPrice:
    input_per_mtok: float
    output_per_mtok: float
    #: Cached-input read rate (typically ~0.1x input). Kept separate so the brake
    #: doesn't over-count re-read prefix tokens.
    cache_read_per_mtok: float


#: USD per 1M tokens. Public list prices (rounded); matched case-insensitively by
#: exact key first, then by the bare model slug. Extend for your providers.
_PRICING: dict[str, ModelPrice] = {
    "anthropic/claude-opus-4-8": ModelPrice(5.0, 25.0, 0.5),
    "anthropic/claude-sonnet-5": ModelPrice(3.0, 15.0, 0.3),
    "anthropic/claude-haiku-4-5": ModelPrice(1.0, 5.0, 0.1),
    "openrouter/qwen/qwen3-coder": ModelPrice(1.0, 3.0, 0.1),
    "openrouter/deepseek/deepseek-chat": ModelPrice(0.5, 1.5, 0.05),
    "openrouter/deepseek/deepseek-r1": ModelPrice(1.0, 3.0, 0.1),
}
#: Used for any model not in the table — conservative (errs toward tripping the
#: brake a little early rather than overrunning the budget).
_FALLBACK = ModelPrice(2.0, 6.0, 0.2)


def price_for(model: str) -> ModelPrice:
    m = model.lower()
    if m in _PRICING:
        return _PRICING[m]
    for key, price in _PRICING.items():
        if key in m or m.endswith(key.rsplit("/", 1)[-1]):
            return price
    return _FALLBACK


def cost_usd(usage: LLMUsage, model: str) -> float:
    """USD cost of one call's token usage. Cache-read tokens are billed at the
    cheap cached rate; the rest of the prompt at the input rate."""
    price = price_for(model)
    cached = max(0, usage.cache_read_tokens)
    fresh_input = max(0, usage.prompt_tokens - cached)
    return (
        fresh_input * price.input_per_mtok
        + cached * price.cache_read_per_mtok
        + usage.completion_tokens * price.output_per_mtok
    ) / _M
