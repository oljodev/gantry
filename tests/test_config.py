from __future__ import annotations

import pytest
from pydantic import ValidationError

from gantry.config import Environment, LogFormat, Settings


def test_defaults() -> None:
    s = Settings(_env_file=None)
    assert s.env is Environment.DEV
    assert s.log_format is LogFormat.CONSOLE
    assert s.database_url_str.startswith("postgresql+asyncpg://")


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
