"""FastAPI application factory for the Gantry control plane.

The server is also the operational home of the **reaper**: the background
loop that re-queues tasks whose worker died mid-lease. Workers deliberately
don't reap (they'd race at scale and it entangles their failure domain);
one control-plane loop with SKIP LOCKED semantics is enough for the fleet.
"""

from __future__ import annotations

import asyncio
import contextlib
import random
from collections.abc import AsyncIterator
from contextlib import asynccontextmanager
from pathlib import Path

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import FileResponse
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry import __version__
from gantry.config import Settings, get_settings
from gantry.core import queue
from gantry.core.db import create_engine, create_session_factory, session_scope
from gantry.logging import configure_logging, get_logger
from gantry.server import agents_api, api, github_api, projects_api, providers_api, ws
from gantry.server.auth import AuthFailed, SupabaseAuthenticator, auth_failed_response, me_router
from gantry.server.broker import EventBroker
from gantry.skills import SkillRegistry
from gantry.vault import Vault

logger = get_logger(__name__)


async def reaper_loop(sessions: async_sessionmaker[AsyncSession], interval_seconds: float) -> None:
    while True:
        await asyncio.sleep(interval_seconds * random.uniform(0.8, 1.2))
        try:
            async with session_scope(sessions) as session:
                reaped = await queue.reap_expired(session)
            if reaped:
                logger.info("server.reaped_expired_leases", count=len(reaped))
        except asyncio.CancelledError:
            raise
        except Exception as exc:  # keep reaping; a blip must not kill the loop
            logger.warning("server.reaper_error", error=repr(exc))


def create_app(settings: Settings | None = None) -> FastAPI:
    settings = settings or get_settings()
    configure_logging(settings)

    @asynccontextmanager
    async def lifespan(app: FastAPI) -> AsyncIterator[None]:
        logger.info("server.starting", env=settings.env, version=__version__)
        engine = create_engine(settings)
        sessions = create_session_factory(engine)
        app.state.sessions = sessions
        async with EventBroker(settings.database_url_str) as broker:
            app.state.broker = broker
            reaper = asyncio.create_task(reaper_loop(sessions, settings.reaper_interval_seconds))
            try:
                yield
            finally:
                reaper.cancel()
                with contextlib.suppress(asyncio.CancelledError):
                    await reaper
                await engine.dispose()
        logger.info("server.stopped")

    app = FastAPI(title="Gantry", version=__version__, lifespan=lifespan)
    app.state.settings = settings
    app.state.skills = SkillRegistry.load_dir(settings.skills_root)
    app.state.auth = (
        SupabaseAuthenticator(
            settings.supabase_url,
            allowed_emails=settings.allowed_emails,
            hs256_secret=settings.supabase_jwt_secret,
        )
        if settings.supabase_url
        else None
    )
    app.state.vault = Vault.from_settings(settings) if settings.vault_key else None
    app.add_exception_handler(AuthFailed, auth_failed_response)  # type: ignore[arg-type]
    app.add_middleware(
        CORSMiddleware,
        allow_origins=settings.cors_origins,
        allow_methods=["*"],
        allow_headers=["*"],
    )
    app.include_router(me_router)
    app.include_router(api.router)
    app.include_router(projects_api.router)
    app.include_router(providers_api.router)
    app.include_router(github_api.router)
    app.include_router(agents_api.router)
    app.include_router(ws.router)

    @app.get("/healthz")
    async def healthz() -> dict[str, str]:
        return {"status": "ok", "env": settings.env, "version": __version__}

    _mount_frontend(app, settings.frontend_dist)
    return app


def _mount_frontend(app: FastAPI, dist: Path) -> None:
    """Serve the built SPA (if present) with a client-route fallback.

    Registered last so /api, /healthz and the WebSocket routes always win.
    In dev the Vite server proxies to us instead and this never mounts.
    """
    root = dist.resolve()
    if not (root / "index.html").is_file():
        return

    @app.get("/{path:path}", include_in_schema=False)
    async def spa(path: str) -> FileResponse:
        candidate = (root / path).resolve()
        if path and candidate.is_file() and candidate.is_relative_to(root):
            return FileResponse(candidate)
        return FileResponse(root / "index.html")
