from __future__ import annotations

import os
from typing import cast

import pytest
from pydantic import ValidationError

from gantry.config import Environment, LogFormat, Settings


def test_defaults(monkeypatch: pytest.MonkeyPatch) -> None:
    # Strip ambient GANTRY_* vars (CI sets GANTRY_ENV=test) to test true defaults.
    for key in list(os.environ):
        if key.startswith("GANTRY_"):
            monkeypatch.delenv(key)
    s = Settings(_env_file=None)
    assert s.env is Environment.DEV
    assert s.log_format is LogFormat.CONSOLE
    assert s.database_url_str.startswith("postgresql+asyncpg://")


async def test_create_engine_sizes_pools_independently() -> None:
    from sqlalchemy.pool import QueuePool

    from gantry.core.db import create_engine

    s = Settings(_env_file=None, db_pool_size=20, db_max_overflow=10)
    # No override -> the shared worker default.
    worker = create_engine(s)
    # Explicit override -> the API's own (smaller) pool, independent of the worker.
    api = create_engine(s, pool_size=5, max_overflow=2)
    try:
        assert cast(QueuePool, worker.pool).size() == 20
        assert cast(QueuePool, api.pool).size() == 5
    finally:
        await worker.dispose()
        await api.dispose()


def test_env_var_override(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("GANTRY_ENV", "prod")
    monkeypatch.setenv("GANTRY_LOG_FORMAT", "json")
    monkeypatch.setenv("GANTRY_DATABASE_URL", "postgresql+asyncpg://u:p@db.example.com:5432/gantry")
    s = Settings(_env_file=None)
    assert s.env is Environment.PROD
    assert s.log_format is LogFormat.JSON
    assert "db.example.com" in s.database_url_str


def test_rejects_sync_database_driver(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("GANTRY_DATABASE_URL", "postgresql://u:p@localhost:5432/gantry")
    with pytest.raises(ValidationError, match="asyncpg"):
        Settings(_env_file=None)


def test_settings_are_frozen() -> None:
    s = Settings(_env_file=None)
    with pytest.raises(ValidationError):
        s.env = Environment.PROD  # type: ignore[misc]
