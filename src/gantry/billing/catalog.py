"""Model list prices — a live OpenRouter snapshot over a static floor.

Two sources, in strict precedence:

1. a **snapshot** fetched from OpenRouter's public model list, refreshed on a
   TTL. OpenRouter publishes per-token USD prices for hundreds of models, which
   is the only practical way to price a slug nobody hard-coded.
2. the **static table**, mirroring ``runtime/pricing.py``, for first-party
   Anthropic slugs and as the answer when the network is unavailable.

Lookup is a **synchronous, pure dict read** against whatever snapshot is loaded.
That is deliberate: pricing sits directly in the path of every LLM response, and
an HTTP call there would put provider latency between a completion and the
charge for it. Refreshing is an explicit background step (:func:`refresh_prices`)
that a long-lived process calls at boot and on the TTL; if it never runs, or
fails, everything still prices off the static table.
"""

from __future__ import annotations

import asyncio
import time
from dataclasses import dataclass
from decimal import Decimal, InvalidOperation
from typing import Any

from gantry.config import get_settings
from gantry.logging import get_logger

logger = get_logger(__name__)


@dataclass(frozen=True)
class TokenPrice:
    """USD per 1M tokens."""

    input_per_mtok: Decimal
    output_per_mtok: Decimal
    #: Cached-input READ rate (typically ~0.1x input).
    cache_read_per_mtok: Decimal
    #: Cache WRITE surcharge (typically ~1.25x input; 0 where unpriced).
    cache_write_per_mtok: Decimal = Decimal(0)


def _price(inp: str, out: str, read: str, write: str = "0") -> TokenPrice:
    return TokenPrice(Decimal(inp), Decimal(out), Decimal(read), Decimal(write))


#: Public list prices, USD per 1M tokens. Keys are matched case-insensitively:
#: exact slug first, then bare-slug containment — the same order as
#: ``runtime/pricing.py``, so the USD brake and the credit ledger never disagree
#: about what a model costs.
_STATIC: dict[str, TokenPrice] = {
    "anthropic/claude-opus-4-8": _price("5.0", "25.0", "0.5", "6.25"),
    "anthropic/claude-sonnet-5": _price("3.0", "15.0", "0.3", "3.75"),
    "anthropic/claude-haiku-4-5": _price("1.0", "5.0", "0.1", "1.25"),
    "openrouter/qwen/qwen3-coder": _price("1.0", "3.0", "0.1"),
    "openrouter/deepseek/deepseek-chat": _price("0.5", "1.5", "0.05"),
    "openrouter/deepseek/deepseek-r1": _price("1.0", "3.0", "0.1"),
}

#: Used for a slug in neither the snapshot nor the table. Conservative on
#: purpose: under-pricing an unknown model silently eats the margin, while
#: over-pricing is visible to the user and correctable.
FALLBACK = _price("2.0", "6.0", "0.2")


class PriceCatalog:
    """Mutable price snapshot shared process-wide. Reads are lock-free."""

    def __init__(self) -> None:
        self._live: dict[str, TokenPrice] = {}
        self._fetched_at: float = 0.0
        self._lock = asyncio.Lock()

    @property
    def live_count(self) -> int:
        return len(self._live)

    @property
    def lock(self) -> asyncio.Lock:
        """Held across a refresh so concurrent callers collapse onto one fetch."""
        return self._lock

    @property
    def fetched_at(self) -> float:
        return self._fetched_at

    def is_stale(self, ttl_seconds: float) -> bool:
        return not self._live or (time.monotonic() - self._fetched_at) > ttl_seconds

    def load(self, prices: dict[str, TokenPrice]) -> None:
        """Install a snapshot (used by the fetcher and by tests)."""
        self._live = prices
        self._fetched_at = time.monotonic()

    def clear(self) -> None:
        self._live = {}
        self._fetched_at = 0.0

    def lookup(self, model: str) -> TokenPrice:
        """Price for a slug: live snapshot, then static table, then fallback."""
        slug = (model or "").strip().lower()
        if not slug:
            return FALLBACK
        # OpenRouter keys its catalog WITHOUT our provider-route prefix, so
        # "openrouter/qwen/qwen3-coder" has to be tried as "qwen/qwen3-coder" too.
        #
        # The live snapshot is exhausted across ALL candidate forms before the
        # static table is consulted at all. Interleaving them would let a
        # hard-coded entry for the fully-prefixed slug beat a freshly fetched
        # price for its bare form — i.e. the six models we happen to have in the
        # table would silently ignore the live feed, which is the one place a
        # stale price actually costs money.
        candidates = _candidates(slug)
        for candidate in candidates:
            live = self._live.get(candidate)
            if live is not None:
                return live
        for candidate in candidates:
            static = _STATIC.get(candidate)
            if static is not None:
                return static
        for key, price in _STATIC.items():
            bare = key.rsplit("/", 1)[-1]
            if key in slug or slug.endswith(bare):
                return price
        return FALLBACK


def _candidates(slug: str) -> tuple[str, ...]:
    """The slug and its de-prefixed forms, most specific first."""
    parts = slug.split("/")
    return tuple("/".join(parts[i:]) for i in range(len(parts)))


#: Process-wide catalog. One snapshot serves every agent in the worker.
CATALOG = PriceCatalog()


def price_for_model(model: str) -> TokenPrice:
    """USD-per-1M price for a model slug (see :meth:`PriceCatalog.lookup`)."""
    return CATALOG.lookup(model)


def parse_openrouter_models(payload: Any) -> dict[str, TokenPrice]:
    """Convert an OpenRouter ``/models`` body into our price table.

    OpenRouter quotes USD **per token** as strings (``"0.000003"``); we store per
    1M. Entries that are missing, unparseable, or free ("0") are skipped rather
    than defaulted — a model priced at zero by a malformed response would be
    billed at zero margin, so a bad row must fall through to the static table
    instead of overriding it.
    """
    data = payload.get("data") if isinstance(payload, dict) else None
    if not isinstance(data, list):
        return {}
    out: dict[str, TokenPrice] = {}
    for entry in data:
        if not isinstance(entry, dict):
            continue
        slug = str(entry.get("id") or "").strip().lower()
        pricing = entry.get("pricing")
        if not slug or not isinstance(pricing, dict):
            continue
        prompt = _per_mtok(pricing.get("prompt"))
        completion = _per_mtok(pricing.get("completion"))
        if prompt is None or completion is None:
            continue
        read = _per_mtok(pricing.get("input_cache_read"))
        write = _per_mtok(pricing.get("input_cache_write"))
        out[slug] = TokenPrice(
            input_per_mtok=prompt,
            output_per_mtok=completion,
            # No published cache rate => assume the provider charges full input
            # for a re-read. Erring high here protects the margin; erring low
            # would hand away the discount we never received.
            cache_read_per_mtok=read if read is not None else prompt,
            cache_write_per_mtok=write if write is not None else Decimal(0),
        )
    return out


def _per_mtok(raw: Any) -> Decimal | None:
    """A per-token price string -> USD per 1M tokens. None if unusable."""
    if raw is None:
        return None
    try:
        per_token = Decimal(str(raw))
    except (InvalidOperation, ValueError):
        return None
    if per_token < 0:
        return None
    return per_token * Decimal(1_000_000)


async def refresh_prices(*, force: bool = False) -> int:
    """Refresh the snapshot from OpenRouter. Returns how many models loaded.

    Never raises: pricing must degrade to the static table rather than take down
    a worker because a public endpoint is slow. Concurrent callers collapse onto
    one fetch via the catalog lock.
    """
    settings = get_settings()
    if not settings.openrouter_pricing_enabled:
        return 0
    if not force and not CATALOG.is_stale(settings.openrouter_pricing_ttl_seconds):
        return CATALOG.live_count

    async with CATALOG.lock:
        if not force and not CATALOG.is_stale(settings.openrouter_pricing_ttl_seconds):
            return CATALOG.live_count
        try:
            import httpx

            async with httpx.AsyncClient(timeout=15.0) as client:
                response = await client.get(settings.openrouter_pricing_url)
                response.raise_for_status()
                prices = parse_openrouter_models(response.json())
        except Exception as exc:
            logger.warning("billing.price_refresh_failed", error=repr(exc))
            return CATALOG.live_count
        if not prices:
            logger.warning("billing.price_refresh_empty")
            return CATALOG.live_count
        CATALOG.load(prices)
        logger.info("billing.prices_refreshed", models=len(prices))
        return len(prices)
