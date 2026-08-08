"""Dynamic effective-cost routing: rank the upstream providers OpenRouter (or
any multi-backend gateway) can route ONE model slug to, by the cost we
actually expect to pay for THIS request — not by nominal list price.

A provider with a higher advertised per-token input price can still be the
cheaper choice once its prompt-cache hit rate is accounted for: a warm cache
serves most of the prompt at the cheap cache-read rate instead of the full
input rate, so a provider that caches well can beat a nominally-cheaper one
that doesn't cache at all — but only once the prompt is large enough for the
cache saving to outweigh a pricier output rate. This module is the pure math
behind that decision; it holds no opinion about WHICH providers exist. Every
provider is an opaque, caller-supplied :class:`ProviderCandidate` — the engine
never branches on a name, so it has nothing to hardcode.

The formula, for a candidate ``k``:

    C_eff_in,k  = (1 - H_k) * P_in,k + H_k * P_cache,k
    Cost_total,k = (T_in * C_eff_in,k + T_out * P_out,k) / 1,000,000

where ``H_k`` is candidate ``k``'s observed prompt-cache hit rate (0..1),
``P_in``/``P_cache``/``P_out`` are USD per 1M tokens, and ``T_in``/``T_out``
are the request's input and estimated output token counts.
"""

from __future__ import annotations

from collections.abc import Sequence
from dataclasses import dataclass
from decimal import Decimal
from typing import Any

#: Tokens per priced unit — every price in this module is USD per 1M tokens,
#: matching how every provider (and gantry.billing.catalog) quotes list price.
MTOK = Decimal(1_000_000)


@dataclass(frozen=True)
class ProviderCandidate:
    """One routable upstream for a model slug: its own pricing and its own
    observed prompt-cache hit rate.

    ``name`` is an opaque label the caller controls (an OpenRouter provider
    slug, a config key, a test fixture id) — this module reads it only to echo
    it back in the ranked result and the routing payload, never to decide
    anything.
    """

    name: str
    price_in_per_mtok: Decimal
    price_cache_per_mtok: Decimal
    price_out_per_mtok: Decimal
    #: H_k: fraction of input tokens expected to be served from cache, 0..1.
    cache_hit_rate: Decimal = Decimal(0)

    def __post_init__(self) -> None:
        if not self.name:
            raise ValueError("a provider candidate needs a non-empty name")
        if not (Decimal(0) <= self.cache_hit_rate <= Decimal(1)):
            raise ValueError(f"cache_hit_rate must be within [0, 1], got {self.cache_hit_rate!r}")
        for field_name, value in (
            ("price_in_per_mtok", self.price_in_per_mtok),
            ("price_cache_per_mtok", self.price_cache_per_mtok),
            ("price_out_per_mtok", self.price_out_per_mtok),
        ):
            if value < 0:
                raise ValueError(f"{field_name} must be non-negative, got {value!r}")


def effective_input_cost_per_mtok(candidate: ProviderCandidate) -> Decimal:
    """C_eff_in,k = (1 - H_k) * P_in,k + H_k * P_cache,k

    The blended per-1M-token input rate once the candidate's cache hit rate is
    priced in: a cache miss costs the full input rate, a cache hit costs the
    (usually much cheaper) cache-read rate.
    """
    h = candidate.cache_hit_rate
    return (Decimal(1) - h) * candidate.price_in_per_mtok + h * candidate.price_cache_per_mtok


def expected_total_cost_usd(
    candidate: ProviderCandidate, input_tokens: int, output_tokens: int
) -> Decimal:
    """Cost_total,k = (T_in * C_eff_in,k + T_out * P_out,k) / 1,000,000

    The expected USD cost of routing this one request to ``candidate``, given
    its prompt size and estimated completion size.
    """
    if input_tokens < 0 or output_tokens < 0:
        raise ValueError("token counts must be non-negative")
    c_eff_in = effective_input_cost_per_mtok(candidate)
    total = Decimal(input_tokens) * c_eff_in + Decimal(output_tokens) * candidate.price_out_per_mtok
    return total / MTOK


@dataclass(frozen=True)
class RankedCandidate:
    """One candidate's place in a ranking, with the cost that put it there."""

    candidate: ProviderCandidate
    expected_cost_usd: Decimal


def rank_candidates(
    candidates: Sequence[ProviderCandidate], input_tokens: int, output_tokens: int
) -> list[RankedCandidate]:
    """Every candidate, cheapest expected cost first.

    A stable sort: candidates tied on cost keep their input order, so ranking
    is deterministic for a fixed candidate list rather than depending on sort
    implementation details.
    """
    ranked = [
        RankedCandidate(c, expected_total_cost_usd(c, input_tokens, output_tokens))
        for c in candidates
    ]
    ranked.sort(key=lambda r: r.expected_cost_usd)
    return ranked


def best_candidate(
    candidates: Sequence[ProviderCandidate], input_tokens: int, output_tokens: int
) -> ProviderCandidate | None:
    """The single cheapest-expected-cost candidate, or None for an empty list."""
    ranked = rank_candidates(candidates, input_tokens, output_tokens)
    return ranked[0].candidate if ranked else None


def provider_routing_payload(
    candidates: Sequence[ProviderCandidate],
    input_tokens: int,
    output_tokens: int,
    *,
    allow_fallbacks: bool = True,
) -> dict[str, Any] | None:
    """The OpenRouter ``provider`` request field, ordered cheapest-expected-cost
    first — ``{"order": [...], "allow_fallbacks": ...}``.

    None for an empty candidate list: with nothing to rank, the request is
    left exactly as it would be without this feature (no preference set)
    rather than emitting an empty, meaningless order.
    """
    ranked = rank_candidates(candidates, input_tokens, output_tokens)
    if not ranked:
        return None
    return {
        "order": [r.candidate.name for r in ranked],
        "allow_fallbacks": allow_fallbacks,
    }
