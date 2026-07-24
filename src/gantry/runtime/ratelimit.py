"""Outbound LLM pacers: one non-blocking token-rate limiter per provider.

With many agents driven concurrently in one event loop, dozens of requests can
fire in the same tick and trip a provider's localized 429s even when the global
RPM budget is fine. A limiter smooths starts to a steady rate with a small burst
allowance: callers ``await acquire()`` immediately before each API call, and
excess callers wait (cheaply) until their slot comes due.

Scheduling is virtual (GCRA): each ``acquire`` reserves the next slot on a shared
deadline (``_tat``) under a brief, await-free lock, then sleeps OUTSIDE the lock
until that slot is due. So N concurrent acquirers each reserve instantly and then
sleep concurrently — unlike a token bucket that holds its lock across the sleep,
whose lock-wait compounds into an O(N) serialized tail under sustained load.

Limiters are keyed by provider base URL (``LimiterRegistry``): every provider
paces on its own budget, so a slow/throttled provider can't stall requests to a
different one. Same-key clients share one limiter, so the whole fleet on a given
provider still paces as a single stream.
"""

from __future__ import annotations

import asyncio


class AsyncRateLimiter:
    """Virtual-scheduling (GCRA) rate limiter over the event-loop clock.

    Sustains ``rate_per_second`` starts with up to ``burst`` allowed back-to-back
    when idle. ``acquire`` advances the shared theoretical-arrival-time under a
    short lock (no ``await`` inside), then sleeps its own deficit outside the lock,
    so waiters neither block one another's reservation nor bank unbounded burst.
    """

    def __init__(self, rate_per_second: float, burst: int | None = None) -> None:
        if rate_per_second <= 0:
            raise ValueError("rate_per_second must be positive")
        #: T — steady-state spacing between starts.
        self._interval = 1.0 / float(rate_per_second)
        capacity = max(1.0, float(burst if burst is not None else max(1.0, rate_per_second)))
        #: tau — how far ahead of the virtual clock a request may run (the burst).
        self._burst_tolerance = (capacity - 1.0) * self._interval
        #: Theoretical arrival time of the next start; bound to the loop clock on
        #: first use. Clamped to ``now`` each call so a long idle can't bank more
        #: than ``burst`` immediate starts.
        self._tat: float | None = None
        self._lock = asyncio.Lock()

    async def acquire(self) -> None:
        loop = asyncio.get_running_loop()
        async with self._lock:  # brief + await-free: reserve this call's slot
            now = loop.time()
            tat = now if self._tat is None else max(self._tat, now)
            allow_at = tat - self._burst_tolerance  # may be <= now while in burst
            self._tat = tat + self._interval
        delay = allow_at - now
        if delay > 0:  # sleep the deficit OUTSIDE the lock so others reserve now
            await asyncio.sleep(delay)


class LimiterRegistry:
    """Process-wide map of provider base URL -> its own ``AsyncRateLimiter``.

    Each distinct provider paces independently at the same configured rate, so one
    provider's throttling never stalls another. Clients with the same base URL (or
    all the keyless/default clients, keyed by ``""``) share one limiter.
    """

    def __init__(self, rate_per_second: float, burst: int | None = None) -> None:
        self._rate = rate_per_second
        self._burst = burst
        self._by_key: dict[str, AsyncRateLimiter] = {}

    def get(self, base_url: str | None) -> AsyncRateLimiter:
        key = base_url or ""
        limiter = self._by_key.get(key)
        if limiter is None:
            limiter = AsyncRateLimiter(self._rate, self._burst)
            self._by_key[key] = limiter
        return limiter
