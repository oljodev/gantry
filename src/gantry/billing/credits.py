"""Raw provider cost -> Gantry Credits, at a guaranteed gross margin.

The chain is deliberately explicit and exact:

    raw_cost_usd = tokens x list price          (Decimal, no float drift)
    charge_usd   = raw_cost_usd / cost_ratio    (the markup)
    credits      = charge_usd x credits_per_usd

``cost_ratio`` is the fraction of revenue that provider cost is allowed to eat,
so it is ``1 - target_margin``: 0.60 means the call may consume 60% of what we
charge, leaving a **40% gross margin**. That is the minimum target, so lowering
the ratio widens the margin and raising it narrows it. The naming matters
because the two numbers are easy to swap and a swap silently inverts the
business model — hence :func:`gross_margin`, which states the realised margin
back in the terms an operator actually reasons about.

Everything here is a **pure function of Decimals**. Money is never a float: a
balance decremented a million times by a float charge drifts away from
``SUM(credits_deducted)``, and a ledger that cannot be re-derived cannot be
audited. Prices arrive as Decimals from :mod:`gantry.billing.catalog`.
"""

from __future__ import annotations

from dataclasses import dataclass
from decimal import ROUND_HALF_UP, Decimal

from gantry.billing.catalog import TokenPrice, price_for_model
from gantry.config import Settings, get_settings

#: Tokens per priced unit. Provider list prices are quoted per 1M tokens.
MTOK = Decimal(1_000_000)

#: Credit amounts are stored NUMERIC(18, 6); quantize to match so what we compute
#: is exactly what Postgres stores (no silent re-rounding on write).
CREDIT_QUANTUM = Decimal("0.000001")
USD_QUANTUM = Decimal("0.00000001")


@dataclass(frozen=True)
class CallCharge:
    """What one LLM call cost and what it is billed at."""

    model_slug: str
    prompt_tokens: int
    completion_tokens: int
    cache_read_tokens: int
    cache_write_tokens: int
    raw_cost_usd: Decimal
    credits_deducted: Decimal

    @property
    def total_tokens(self) -> int:
        """Billable token count. Cache reads/writes are already counted inside
        ``prompt_tokens`` by every provider we support, so they are NOT added
        again here — doing so would double-count a cache-warm agent's prompt."""
        return self.prompt_tokens + self.completion_tokens


def gross_margin(cost_ratio: float) -> float:
    """The gross margin a cost ratio yields, as a fraction. ``0.60 -> 0.40``.

    The inverse of the knob, provided so callers (and logs) can talk about the
    margin they are targeting rather than the ratio that produces it.
    """
    return 1.0 - cost_ratio


def _cost_ratio(settings: Settings) -> Decimal:
    """The configured cost ratio, clamped to a sane open interval.

    A ratio of 0 would divide by zero (infinite charge) and a ratio above 1 would
    bill BELOW cost — every call a guaranteed loss. Both are configuration
    mistakes rather than states to propagate, so they are clamped here, at the
    one place the number is consumed.
    """
    ratio = Decimal(str(settings.credit_cost_ratio))
    if ratio <= 0:
        return Decimal("0.6")
    return min(ratio, Decimal(1))


def raw_cost_usd(
    price: TokenPrice,
    *,
    prompt_tokens: int,
    completion_tokens: int,
    cache_read_tokens: int = 0,
    cache_write_tokens: int = 0,
) -> Decimal:
    """Provider list cost of one call, in USD.

    Cache-read tokens are billed at the cheap cached rate and subtracted from the
    fresh input they are part of, mirroring how providers actually invoice: a
    prompt-cached agent whose 40K-token prefix is a hit must not be charged 40K
    fresh input tokens, or the margin engine would bill users for a discount we
    received.
    """
    cached = max(0, cache_read_tokens)
    written = max(0, cache_write_tokens)
    fresh_input = max(0, prompt_tokens - cached)
    total = (
        Decimal(fresh_input) * price.input_per_mtok
        + Decimal(cached) * price.cache_read_per_mtok
        + Decimal(written) * price.cache_write_per_mtok
        + Decimal(max(0, completion_tokens)) * price.output_per_mtok
    ) / MTOK
    return total.quantize(USD_QUANTUM, rounding=ROUND_HALF_UP)


def credits_for_usd(cost_usd: Decimal, settings: Settings | None = None) -> Decimal:
    """Mark a raw USD cost up to the margin target and convert it to credits."""
    cfg = settings or get_settings()
    charge = cost_usd / _cost_ratio(cfg)
    credits = charge * Decimal(str(cfg.credits_per_usd))
    return credits.quantize(CREDIT_QUANTUM, rounding=ROUND_HALF_UP)


def calculate_credits_for_call(
    model_slug: str,
    prompt_tokens: int,
    completion_tokens: int,
    *,
    cache_read_tokens: int = 0,
    cache_write_tokens: int = 0,
    settings: Settings | None = None,
) -> CallCharge:
    """Price one LLM call: list cost, then the marked-up credit charge.

    The three positional arguments are the whole contract — a caller that knows
    nothing about prompt caching gets a correct (if slightly conservative) charge.
    The cache keyword arguments are how the metering path passes the breakdown
    providers actually report, so a cache hit is billed as a cache hit.

    Pure and deterministic: same slug and same token counts always produce the
    same charge, which is what makes the ledger reproducible from the event log.
    """
    cfg = settings or get_settings()
    price = price_for_model(model_slug)
    cost = raw_cost_usd(
        price,
        prompt_tokens=prompt_tokens,
        completion_tokens=completion_tokens,
        cache_read_tokens=cache_read_tokens,
        cache_write_tokens=cache_write_tokens,
    )
    return CallCharge(
        model_slug=model_slug,
        prompt_tokens=max(0, prompt_tokens),
        completion_tokens=max(0, completion_tokens),
        cache_read_tokens=max(0, cache_read_tokens),
        cache_write_tokens=max(0, cache_write_tokens),
        raw_cost_usd=cost,
        credits_deducted=credits_for_usd(cost, cfg),
    )
