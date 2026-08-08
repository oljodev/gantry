"""The learning half of dynamic cost routing: turning actual metered calls
into the ``H_k`` (cache hit rate) that :mod:`gantry.runtime.routing` prices
candidates with.

Every metered call already lands a row in ``llm_usage_logs`` with its real
``prompt_tokens``/``cache_read_tokens`` split (``billing/metering.py`` writes
one per call, unconditionally). Nothing new needs to be recorded to learn a
hit rate — it is already sitting in the audit trail; this module just
aggregates the most recent rows for a model into a single ratio. That ratio
tracks a live model/provider automatically: if a provider starts or stops
serving a request from cache, the rolling window ages the old behaviour out
within ``limit`` calls, with no separate "retrain" step.
"""

from __future__ import annotations

from decimal import Decimal

import sqlalchemy as sa
from sqlalchemy.ext.asyncio import AsyncSession

from gantry.core.models import LlmUsageLog

#: Calls considered "recent enough to describe current behaviour". Small on
#: purpose: a provider that flips caching on or off should be reflected in the
#: rate within a handful of calls, not diluted by months of history.
DEFAULT_WINDOW = 50

_QUANTUM = Decimal("0.000001")


async def rolling_cache_hit_rate(
    session: AsyncSession, model_slug: str, *, limit: int = DEFAULT_WINDOW
) -> Decimal | None:
    """H_k for ``model_slug``: ``sum(cache_read_tokens) / sum(prompt_tokens)``
    over its most recent ``limit`` billed calls.

    None when there is no billing history for this slug yet — a router with no
    signal should fall back to a conservative default (H_k = 0, i.e. "assume no
    caching") rather than a query returning zero being mistaken for "we
    observed zero cache hits".
    """
    recent = (
        sa.select(LlmUsageLog.prompt_tokens, LlmUsageLog.cache_read_tokens)
        .where(LlmUsageLog.model_slug == model_slug)
        .order_by(LlmUsageLog.created_at.desc())
        .limit(limit)
        .subquery()
    )
    totals = (
        await session.execute(
            sa.select(
                sa.func.coalesce(sa.func.sum(recent.c.prompt_tokens), 0),
                sa.func.coalesce(sa.func.sum(recent.c.cache_read_tokens), 0),
            )
        )
    ).one()
    total_prompt, total_cache_read = totals
    if not total_prompt:
        return None
    return (Decimal(total_cache_read) / Decimal(total_prompt)).quantize(_QUANTUM)
