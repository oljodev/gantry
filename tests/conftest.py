from __future__ import annotations

import asyncio
import os
import socket
import subprocess
import sys
from collections.abc import AsyncIterator
from pathlib import Path

import asyncpg
import httpx
import pytest
import sqlalchemy as sa
from fastapi import FastAPI
from sqlalchemy.engine import make_url
from sqlalchemy.ext.asyncio import (
    AsyncEngine,
    AsyncSession,
    async_sessionmaker,
    create_async_engine,
)
from sqlalchemy.pool import NullPool

from gantry.config import Settings
from gantry.core.notify import asyncpg_dsn

REPO_ROOT = Path(__file__).resolve().parent.parent


@pytest.fixture
def settings() -> Settings:
    # _env_file=None keeps a developer's local .env from leaking into tests.
    return Settings(_env_file=None)


# --- Postgres fixtures ---------------------------------------------------
#
# Tests always run against a database named *_test. In CI, GANTRY_DATABASE_URL
# already points at one; locally we derive it from the configured URL (so a
# stray `pytest` can never touch the dev database) and create it on demand.


def _test_database_url() -> str:
    url = make_url(Settings(_env_file=None).database_url_str)
    if not str(url.database or "").endswith("_test"):
        url = url.set(database=f"{url.database}_test")
    return url.render_as_string(hide_password=False)


def _postgres_reachable(url: str) -> bool:
    u = make_url(url)
    try:
        with socket.create_connection((u.host or "localhost", u.port or 5432), timeout=2):
            return True
    except OSError:
        return False


async def _ensure_database_exists(url: str) -> None:
    u = make_url(url)
    admin = u.set(database="postgres")
    conn = await asyncpg.connect(asyncpg_dsn(admin.render_as_string(hide_password=False)))
    try:
        exists = await conn.fetchval("SELECT 1 FROM pg_database WHERE datname = $1", u.database)
        if not exists:
            await conn.execute(f'CREATE DATABASE "{u.database}"')
    finally:
        await conn.close()


async def _reset_schema(url: str) -> None:
    conn = await asyncpg.connect(asyncpg_dsn(url))
    try:
        await conn.execute("DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
    finally:
        await conn.close()


@pytest.fixture(scope="session")
def database_url() -> str:
    """Migrated, empty *_test database; skips the test if Postgres is down."""
    url = _test_database_url()
    if not _postgres_reachable(url):
        pytest.skip(
            "Postgres is not reachable — start it with `make dev` "
            f"(wanted {make_url(url).host}:{make_url(url).port or 5432})"
        )
    asyncio.run(_ensure_database_exists(url))
    asyncio.run(_reset_schema(url))
    subprocess.run(
        [sys.executable, "-m", "alembic", "upgrade", "head"],
        cwd=REPO_ROOT,
        env={**os.environ, "GANTRY_DATABASE_URL": url},
        check=True,
        capture_output=True,
    )
    return url


@pytest.fixture
async def engine(database_url: str) -> AsyncIterator[AsyncEngine]:
    engine = create_async_engine(database_url, poolclass=NullPool)
    # Each test starts from clean tables (schema itself persists per session).
    async with engine.begin() as conn:
        await conn.execute(
            sa.text(
                "TRUNCATE tasks, task_events, secrets, providers, agent_profiles, "
                "teams, team_members, skills, copilot_sessions, attachments, projects, "
                "workspace_controls "
                "RESTART IDENTITY CASCADE"
            )
        )
        # Re-seed the Default project the migration created — enqueue() and the
        # backfill convention both depend on DEFAULT_PROJECT_ID existing.
        await conn.execute(
            sa.text(
                "INSERT INTO projects (id, workspace_id, name, description) "
                "VALUES ('00000000-0000-0000-0000-000000000002', "
                "'00000000-0000-0000-0000-000000000001', 'Default', 'Default project')"
            )
        )
    yield engine
    await engine.dispose()


@pytest.fixture
def db(engine: AsyncEngine) -> async_sessionmaker[AsyncSession]:
    return async_sessionmaker(engine, expire_on_commit=False)


# --- Control plane fixtures ----------------------------------------------
#
# The app runs with its full lifespan (own engine, event broker, reaper) in
# the test's event loop. One httpx client speaks both HTTP and WebSocket
# through the ASGI transport — no sockets, no uvicorn.


@pytest.fixture
async def app(
    engine: AsyncEngine, database_url: str, tmp_path_factory: pytest.TempPathFactory
) -> AsyncIterator[FastAPI]:
    from gantry.server.app import create_app

    settings = Settings(
        _env_file=None,
        database_url=database_url,
        reaper_interval_seconds=0.2,
        # Per-test blob root, so an upload test never writes to a shared dir.
        attachment_root=tmp_path_factory.mktemp("attachments"),
        # Fixed test vault key so secret-carrying routes work.
        vault_key="11" * 32,
        # Auth OFF, explicitly. `_env_file=None` only ignores the .env file —
        # pydantic-settings still reads os.environ, and litellm loads .env into
        # it on import, so a developer with GANTRY_SUPABASE_URL set would
        # otherwise flip auth on and 401 the whole suite. Tests that want auth
        # inject a fake authenticator via app.state.auth instead.
        supabase_url=None,
        allowed_emails=[],
    )
    application = create_app(settings)
    # Drive the lifespan directly (it is a plain asynccontextmanager, safe to
    # exit from a different task — which pytest-asyncio fixture teardown does;
    # asgi-lifespan's anyio cancel scopes are not).
    async with application.router.lifespan_context(application):
        yield application


@pytest.fixture
async def client(app: FastAPI) -> AsyncIterator[httpx.AsyncClient]:
    # ASGIWebSocketTransport holds an anyio task group, whose cancel scope must
    # enter and exit in the SAME task. pytest-asyncio tears fixtures down in a
    # different task, so a plain `async with` here would blow up — instead the
    # client's whole lifecycle runs inside one dedicated task.
    from httpx_ws.transport import ASGIWebSocketTransport

    started: asyncio.Future[httpx.AsyncClient] = asyncio.get_running_loop().create_future()
    closing = asyncio.Event()

    async def lifecycle() -> None:
        transport = ASGIWebSocketTransport(app=app)
        async with httpx.AsyncClient(transport=transport, base_url="http://gantry.test") as c:
            started.set_result(c)
            await closing.wait()

    runner = asyncio.ensure_future(lifecycle())
    yield await started
    closing.set()
    await runner
