"""FastAPI application factory for the Gantry control plane."""

from __future__ import annotations

from collections.abc import AsyncIterator
from contextlib import asynccontextmanager

from fastapi import FastAPI

from gantry import __version__
from gantry.config import Settings, get_settings
from gantry.logging import configure_logging, get_logger

logger = get_logger(__name__)


def create_app(settings: Settings | None = None) -> FastAPI:
    settings = settings or get_settings()
    configure_logging(settings)

    @asynccontextmanager
    async def lifespan(_: FastAPI) -> AsyncIterator[None]:
        logger.info("server.starting", env=settings.env, version=__version__)
        yield
        logger.info("server.stopped")

    app = FastAPI(title="Gantry", version=__version__, lifespan=lifespan)
    app.state.settings = settings

    @app.get("/healthz")
    async def healthz() -> dict[str, str]:
        return {"status": "ok", "env": settings.env, "version": __version__}

    return app
