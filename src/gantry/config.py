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
    #: Insert Anthropic prompt-cache breakpoints into each LLM request. The
    #: durable loop's history is append-only, so the prompt prefix is stable
    #: and re-read at cache rates every step — the main lever against input
    #: token bloat. Kill-switch: GANTRY_PROMPT_CACHING=false.
    prompt_caching: bool = True
    #: Root directory for per-task worker workspaces.
    workspace_root: Path = Path("/tmp/gantry-workspaces")
    #: GitHub token for workers' git operations (vault-managed from Phase 9).
    github_token: str | None = None
    #: Concurrent agent task-slots per worker process. The worker drives this
    #: many agent loops on one asyncio event loop (agents spend ~all their time
    #: awaiting network I/O), so one lightweight process replaces a fleet of OS
    #: processes. Override via GANTRY_WORKER_CONCURRENCY.
    worker_concurrency: int = 100
    #: Async SQLAlchemy pool bounds for ONE process. All task-slots share this one
    #: lean pool (sessions are opened per-checkpoint and returned immediately), so a
    #: few dozen backends serve hundreds of concurrent agents. The worker AND the API
    #: each open their own engine, and running N worker PROCESSES (GANTRY_WORKERS)
    #: multiplies connections: budget ``(worker + API pools) x GANTRY_WORKERS`` well
    #: under pgserver's max_connections (~100, minus reserved/NOTIFY/reaper). Prefer
    #: scaling one process via worker_concurrency + llm_max_rps over many processes.
    #: Override via GANTRY_DB_POOL_SIZE / GANTRY_DB_MAX_OVERFLOW.
    db_pool_size: int = 20
    db_max_overflow: int = 10
    #: Optional API-only pool bounds (the API is request-driven and usually needs
    #: fewer connections than the worker's agent fleet). None -> use the shared
    #: db_pool_size/db_max_overflow. Override via GANTRY_API_DB_POOL_SIZE /
    #: GANTRY_API_DB_MAX_OVERFLOW to size the two engines independently.
    api_db_pool_size: int | None = None
    api_db_max_overflow: int | None = None
    #: Model used to resolve git merge conflicts when integrating swarm branches
    #: — one lightweight turn per conflicted file. None = reuse the leader's own
    #: model. Point it at a cheap/fast model (e.g. a Qwen3/flash) via
    #: GANTRY_CONFLICT_RESOLVER_MODEL; it must be served by the run's provider.
    conflict_resolver_model: str | None = None
    #: Outbound LLM pacing: sustained requests/second and the burst allowance,
    #: shared process-wide across every provider client. Keeps a swarm of agents
    #: from dumping calls into one tick and tripping localized 429s. This is a HARD
    #: process-wide throughput ceiling (a 100-child burst can't exceed it), so size
    #: it to the provider key's real budget. Override via GANTRY_LLM_MAX_RPS /
    #: GANTRY_LLM_RPS_BURST (keep burst ~2x rps so a spike can't exceed the key's
    #: instant limit).
    llm_max_rps: float = 40.0
    llm_rps_burst: int = 80
    #: How often the control plane re-queues tasks whose lease expired.
    reaper_interval_seconds: float = 10.0
    #: Built frontend to serve as the SPA (skipped if index.html is absent).
    frontend_dist: Path = Path("web/dist")
    #: Runaway-planner guard: max children one task may spawn.
    max_subtasks_per_task: int = 32
    #: Default per-agent step budget when a task/profile doesn't set its own.
    #: The ceiling before a run fails with "exceeded max_steps"; override via
    #: GANTRY_DEFAULT_MAX_STEPS or per-agent max_steps.
    default_max_steps: int = 300
    #: Estimated-token ceiling before the durable loop compacts history into a
    #: summary checkpoint. Lower than the old hardcoded 120_000 so per-call input
    #: stays bounded on long runs (compaction fires ~every 10-15 steps instead of
    #: almost never). The fallback for any role without its own budget below.
    #: Override via GANTRY_MAX_CONTEXT_TOKENS.
    max_context_tokens: int = 50_000
    #: Per-role compaction budgets, selected from the durable task kind/payload so a
    #: resume picks the same threshold. A leaf EXECUTE worker runs a small, bounded
    #: micro-task (read a slice, write a file, test) whose live context stays under a
    #: tight cap — so it NEVER compacts and its implicit prefix cache stays warm the
    #: whole task. A delegating leader legitimately accumulates (children reports,
    #: successive waves), so it gets a larger cap and compacts infrequently rather
    #: than every step. A per-task ``max_context_tokens`` payload value overrides
    #: both. Override via GANTRY_EXECUTE_MAX_CONTEXT_TOKENS / GANTRY_LEADER_MAX_CONTEXT_TOKENS.
    execute_max_context_tokens: int = 30_000
    leader_max_context_tokens: int = 80_000
    #: Messages kept verbatim as the recent tail when compaction fires (the rest
    #: is summarized). Keep max_context_tokens >= ~2x the tail's token size or
    #: compaction re-fires every step. Override via GANTRY_KEEP_RECENT_MESSAGES.
    keep_recent_messages: int = 6
    #: Token budget for the verbatim recent tail a size-aware compaction keeps. It
    #: bounds the post-compaction floor so compaction fires every N steps (letting
    #: the provider's implicit prefix cache re-warm) instead of possibly every step.
    #: ~a third of the soft cap. Override via GANTRY_KEEP_RECENT_TOKENS.
    keep_recent_tokens: int = 16_000
    #: Absolute per-step input ceiling. A step whose (elided) context still exceeds
    #: this after normal compaction triggers emergency compaction — making a
    #: 971K-token step impossible by invariant. Override via GANTRY_HARD_MAX_CONTEXT_TOKENS.
    hard_max_context_tokens: int = 96_000
    #: Directory of SKILL.md files loaded by workers and the API.
    skills_root: Path = Path("skills")
    #: Origins allowed to call the API from a browser. Includes the Cloudflare
    #: Pages production origin so a split deploy (static frontend + remote
    #: backend) works out of the box; override via GANTRY_CORS_ORIGINS.
    cors_origins: list[str] = Field(
        default=[
            "http://localhost:5173",
            "http://127.0.0.1:5173",
            "https://gantry.oljo.dev",
        ],
    )
    #: 32-byte AES-256-GCM key (hex or base64) for the secrets vault. Only
    #: validated when a secret operation is attempted, so dev/tests without
    #: secrets never need it. Generated into .env by run.sh.
    vault_key: str | None = None
    #: Supabase project URL (e.g. https://xyz.supabase.co). When set, every
    #: /api route and WebSocket requires a valid Supabase JWT; when None,
    #: auth is disabled entirely (dev/test parity).
    supabase_url: str | None = None
    #: Escape hatch for legacy Supabase projects signing JWTs with HS256.
    supabase_jwt_secret: str | None = None
    #: Emails allowed through auth (case-insensitive). Empty = any valid JWT.
    allowed_emails: list[str] = Field(default_factory=list)

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
