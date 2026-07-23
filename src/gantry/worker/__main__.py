"""Worker entrypoint: ``python -m gantry.worker`` (also the Docker CMD)."""

from __future__ import annotations

import asyncio
import contextlib
import signal

from sqlalchemy.ext.asyncio import async_sessionmaker

from gantry.config import get_settings
from gantry.core.db import create_engine
from gantry.core.notify import TASK_CANCEL_CHANNEL, QueueListener
from gantry.logging import configure_logging, get_logger
from gantry.runtime.llm import LiteLLMClient
from gantry.runtime.ratelimit import AsyncRateLimiter
from gantry.vault import Vault
from gantry.worker.service import Worker, WorkerConfig

logger = get_logger(__name__)


async def main() -> None:
    settings = get_settings()
    configure_logging(settings)
    engine = create_engine(settings)
    sessions = async_sessionmaker(engine, expire_on_commit=False)
    config = WorkerConfig.from_settings(settings)

    shutdown = asyncio.Event()
    loop = asyncio.get_running_loop()
    for sig in (signal.SIGTERM, signal.SIGINT):
        loop.add_signal_handler(sig, shutdown.set)

    vault = Vault.from_settings(settings) if settings.vault_key else None
    # One process-wide outbound pacer, shared by the default client and every
    # per-provider client the worker builds at claim time.
    limiter = AsyncRateLimiter(settings.llm_max_rps, burst=settings.llm_rps_burst)
    async with (
        QueueListener(settings.database_url_str) as listener,
        QueueListener(settings.database_url_str, channel=TASK_CANCEL_CHANNEL) as cancel_listener,
    ):
        worker = Worker(
            sessions,
            config,
            LiteLLMClient(limiter=limiter),
            listener=listener,
            vault=vault,
            limiter=limiter,
            cancel_listener=cancel_listener,
        )
        logger.info(
            "worker.booting",
            concurrency=config.concurrency,
            llm_max_rps=settings.llm_max_rps,
        )
        with contextlib.suppress(asyncio.CancelledError):
            await worker.run(shutdown)
    await engine.dispose()


if __name__ == "__main__":
    asyncio.run(main())
