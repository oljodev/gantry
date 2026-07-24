"""SQLAlchemy declarative base and async engine/session factories."""

from __future__ import annotations

from collections.abc import AsyncIterator
from contextlib import asynccontextmanager

from sqlalchemy.ext.asyncio import (
    AsyncEngine,
    AsyncSession,
    async_sessionmaker,
    create_async_engine,
)
from sqlalchemy.orm import DeclarativeBase

from gantry.config import Settings


class Base(DeclarativeBase):
    """Root of Gantry's ORM metadata; Alembic autogenerate targets this."""


def create_engine(
    settings: Settings,
    *,
    pool_size: int | None = None,
    max_overflow: int | None = None,
) -> AsyncEngine:
    """An async engine. The worker and the API each create their own, so pass
    role-specific ``pool_size``/``max_overflow`` to size them independently (the
    API is request-driven and usually needs fewer than the worker's agent fleet);
    ``None`` falls back to the shared ``db_pool_size``/``db_max_overflow``. Budget
    ``(worker + API pools) x GANTRY_WORKERS`` well under pgserver max_connections."""
    return create_async_engine(
        settings.database_url_str,
        pool_pre_ping=True,
        pool_size=pool_size if pool_size is not None else settings.db_pool_size,
        max_overflow=max_overflow if max_overflow is not None else settings.db_max_overflow,
    )


def create_session_factory(engine: AsyncEngine) -> async_sessionmaker[AsyncSession]:
    return async_sessionmaker(engine, expire_on_commit=False)


@asynccontextmanager
async def session_scope(
    factory: async_sessionmaker[AsyncSession],
) -> AsyncIterator[AsyncSession]:
    """One transactional unit of work: commit on success, rollback on error."""
    async with factory() as session:
        try:
            yield session
            await session.commit()
        except BaseException:
            await session.rollback()
            raise
