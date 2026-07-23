"""The shared outbound pacer: a burst is allowed, then starts are metered."""

from __future__ import annotations

import asyncio

import pytest

from gantry.runtime.ratelimit import AsyncRateLimiter


async def test_burst_is_immediate_then_starts_are_paced() -> None:
    # 200 tokens/sec => one every 5ms once the 3-token burst is spent.
    limiter = AsyncRateLimiter(rate_per_second=200.0, burst=3)
    loop = asyncio.get_running_loop()

    start = loop.time()
    for _ in range(3):  # the burst: effectively free
        await limiter.acquire()
    assert loop.time() - start < 0.02

    start = loop.time()
    for _ in range(4):  # 4 more => 4 * 5ms of enforced pacing
        await limiter.acquire()
    elapsed = loop.time() - start
    assert elapsed >= 0.015  # can't be faster than the metered sleeps


async def test_concurrent_acquirers_are_serialized_at_the_rate() -> None:
    # Many agents hitting the limiter in the same tick must not all fire at once.
    limiter = AsyncRateLimiter(rate_per_second=100.0, burst=1)
    loop = asyncio.get_running_loop()
    start = loop.time()
    await asyncio.gather(*(limiter.acquire() for _ in range(5)))
    # 1 free + 4 paced at 10ms => at least ~40ms regardless of arrival order.
    assert loop.time() - start >= 0.03


def test_rate_must_be_positive() -> None:
    with pytest.raises(ValueError):
        AsyncRateLimiter(rate_per_second=0)
