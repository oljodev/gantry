"""Typed, environment-driven application settings.

All configuration comes from ``GANTRY_``-prefixed environment variables (or a
local ``.env`` file). Settings objects are immutable; use ``get_settings()``
for the process-wide cached instance.
"""

from __future__ import annotations

from enum import StrEnum
from functools import lru_cache
from pathlib import Path

from pydantic import Field, PostgresDsn, field_validator
from pydantic_settings import BaseSettings, SettingsConfigDict


class Environment(StrEnum):
    DEV = "dev"
    TEST = "test"
    PROD = "prod"


class LogFormat(StrEnum):
    CONSOLE = "console"
    JSON = "json"


class Settings(BaseSettings):
    model_config = SettingsConfigDict(
        env_prefix="GANTRY_",
        env_file=".env",
        env_file_encoding="utf-8",
        extra="ignore",
        frozen=True,
    )

    env: Environment = Environment.DEV
    database_url: PostgresDsn = Field(
        default=PostgresDsn("postgresql+asyncpg://gantry:gantry@localhost:5432/gantry"),
    )
    log_level: str = "INFO"
    log_format: LogFormat = LogFormat.CONSOLE
    #: LiteLLM model string (provider-prefixed), overridable per task payload.
    default_model: str = "anthropic/claude-opus-4-8"
    #: Root directory for per-task worker workspaces.
    workspace_root: Path = Path("/tmp/gantry-workspaces")
    #: GitHub token for workers' git operations (vault-managed from Phase 9).
    github_token: str | None = None
    #: How often the control plane re-queues tasks whose lease expired.
    reaper_interval_seconds: float = 10.0
    #: Origins allowed to call the API from a browser (the Phase 5 frontend).
    cors_origins: list[str] = Field(
        default=["http://localhost:5173", "http://127.0.0.1:5173"],
    )

    @field_validator("database_url")
    @classmethod
    def _require_asyncpg_driver(cls, v: PostgresDsn) -> PostgresDsn:
        # The whole stack is asyncio-native; a sync driver here would fail at
        # runtime in ways that are much harder to diagnose than this.
        if v.scheme != "postgresql+asyncpg":
            raise ValueError("database_url must use the postgresql+asyncpg:// scheme")
        return v

    @property
    def database_url_str(self) -> str:
        return str(self.database_url)


@lru_cache
def get_settings() -> Settings:
    return Settings()
