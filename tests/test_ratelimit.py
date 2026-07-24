"""The shared outbound pacer: a burst is allowed, then starts are metered."""

from __future__ import annotations

import asyncio

import pytest

from gantry.runtime.ratelimit import AsyncRateLimiter, LimiterRegistry


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


async def test_concurrent_acquirers_reserve_then_sleep_concurrently() -> None:
    # The GCRA fix: 20 acquirers reserve their slots under a brief lock and then
    # sleep concurrently, so wall-clock tracks the LONGEST deficit (~last slot),
    # not the sum of every prior sleep (which a lock-across-sleep bucket incurs).
    limiter = AsyncRateLimiter(rate_per_second=100.0, burst=1)  # 10ms spacing
    loop = asyncio.get_running_loop()
    start = loop.time()
    await asyncio.gather(*(limiter.acquire() for _ in range(20)))
    elapsed = loop.time() - start
    # 19 paced slots * 10ms = ~190ms of pacing; concurrency keeps it near that
    # floor rather than ballooning, and it can't be faster than the rate.
    assert 0.18 <= elapsed < 0.5


async def test_a_long_idle_does_not_bank_unbounded_burst() -> None:
    # After a long idle the virtual clock is clamped to now, so only `burst`
    # starts are immediate — an idle limiter can't dump a giant burst.
    limiter = AsyncRateLimiter(rate_per_second=50.0, burst=3)  # 20ms spacing
    await limiter.acquire()  # bind the clock, then sit idle
    await asyncio.sleep(0.1)
    loop = asyncio.get_running_loop()
    start = loop.time()
    for _ in range(3):  # exactly `burst` fire immediately
        await limiter.acquire()
    assert loop.time() - start < 0.01
    start = loop.time()
    await limiter.acquire()  # the 4th must wait ~one interval
    assert loop.time() - start >= 0.015


def test_rate_must_be_positive() -> None:
    with pytest.raises(ValueError):
        AsyncRateLimiter(rate_per_second=0)


def test_registry_keys_limiters_by_base_url() -> None:
    registry = LimiterRegistry(rate_per_second=40.0, burst=80)
    a = registry.get("https://api.provider-a.example")
    b = registry.get("https://api.provider-b.example")
    default = registry.get(None)
    assert a is not b  # distinct providers pace independently
    assert a is registry.get("https://api.provider-a.example")  # same key shares one
    assert default is registry.get("")  # keyless/default all share the "" limiter
