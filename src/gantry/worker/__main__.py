"""Worker entrypoint: ``python -m gantry.worker`` (also the Docker CMD)."""

from __future__ import annotations

import asyncio
import contextlib
import signal

from sqlalchemy.ext.asyncio import async_sessionmaker

from gantry.config import get_settings
from gantry.core.db import create_engine
from gantry.core.notify import QueueListener
from gantry.logging import configure_logging, get_logger
from gantry.runtime.llm import LiteLLMClient
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
    async with QueueListener(settings.database_url_str) as listener:
        worker = Worker(sessions, config, LiteLLMClient(), listener=listener, vault=vault)
        with contextlib.suppress(asyncio.CancelledError):
            await worker.run(shutdown)
    await engine.dispose()


if __name__ == "__main__":
    asyncio.run(main())
