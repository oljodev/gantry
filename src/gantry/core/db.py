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


def create_engine(settings: Settings) -> AsyncEngine:
    return create_async_engine(settings.database_url_str, pool_pre_ping=True)


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
