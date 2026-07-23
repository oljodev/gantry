"""A shared async token-bucket limiter that paces outbound LLM dispatch.

With many agents driven concurrently in one event loop, dozens of requests can
fire in the same tick and trip a provider's localized 429s even when the global
RPM budget is fine. This limiter smooths starts to a steady rate with a small
burst allowance: callers ``await acquire()`` immediately before each API call,
and excess callers queue (cheaply, at zero compute) until a token is available.

One instance is shared process-wide across every LLM client — the default one
and the per-provider ones built at claim time — so the whole fleet paces as a
single stream rather than each client pacing itself.
"""

from __future__ import annotations

import asyncio


class AsyncRateLimiter:
    """Token bucket over the event-loop clock.

    ``rate_per_second`` tokens refill continuously up to ``burst`` capacity.
    ``acquire`` serializes waiters (FIFO, via the lock) so that once the burst
    is spent, starts are handed out one per ``1 / rate`` seconds.
    """

    def __init__(self, rate_per_second: float, burst: int | None = None) -> None:
        if rate_per_second <= 0:
            raise ValueError("rate_per_second must be positive")
        self._rate = float(rate_per_second)
        self._capacity = float(burst if burst is not None else max(1.0, rate_per_second))
        self._tokens = self._capacity
        self._updated: float | None = None
        self._lock = asyncio.Lock()

    async def acquire(self) -> None:
        async with self._lock:
            loop = asyncio.get_running_loop()
            while True:
                now = loop.time()
                if self._updated is None:  # first call binds to the running loop's clock
                    self._updated = now
                self._tokens = min(
                    self._capacity, self._tokens + (now - self._updated) * self._rate
                )
                self._updated = now
                if self._tokens >= 1.0:
                    self._tokens -= 1.0
                    return
                # Hold the lock while sleeping the exact deficit — this both
                # paces the stream and keeps waiters strictly ordered.
                await asyncio.sleep((1.0 - self._tokens) / self._rate)
