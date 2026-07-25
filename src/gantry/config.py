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
    #: Repair-wave breaker: once this many of a leader's DIRECT children have
    #: terminally FAILED, the spawn tools refuse to launch more — a leader stuck
    #: spawning fixer wave after fixer wave must integrate/land what already works,
    #: or abort and report, never keep funding the same failing approach. Counts
    #: failures only, so a healthy build of many succeeding children never trips it.
    #: Override via GANTRY_MAX_REPAIR_FAILURES.
    max_repair_failures: int = 5
    #: Run-wide structural backstop: the spawn tools refuse once the whole tree
    #: (root_task_id) already holds this many tasks — bounding total fan-out
    #: regardless of nesting depth (max_subtasks_per_task is per-parent and
    #: multiplies with the tree). Raise it with the budget for a large build.
    #: Override via GANTRY_RUN_TASK_CEILING.
    run_task_ceiling: int = 50
    #: Default per-agent step budget for a STANDALONE / interactive leaf worker (a
    #: user's single task, not a spawned micro-task). The ceiling before the run
    #: fails with "exceeded max_steps"; override via GANTRY_DEFAULT_MAX_STEPS or a
    #: per-agent max_steps.
    default_max_steps: int = 300
    #: Circuit-breaker step caps, selected by role from the durable kind/payload so a
    #: resume picks the same ceiling (mirrors the per-role compaction budgets below).
    #: A spawned autonomous-swarm LEAF (non-interactive micro-task) is handed one
    #: file + one outcome, so anything past a handful of steps is stuck — fail it so
    #: the leader learns rather than letting it grind. A DELEGATING agent (plan task /
    #: can_spawn / autonomous leader) gets a generous but FINITE budget (never
    #: unbounded — even unattended it must halt); when it trips it halts GRACEFULLY
    #: with a report, not a FAIL. The run's live cost/emergency-stop budget is the
    #: primary spend bound; these are structural backstops. Override via
    #: GANTRY_EXECUTE_MAX_STEPS / GANTRY_LEADER_MAX_STEPS.
    execute_max_steps: int = 25
    leader_max_steps: int = 150
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
    #: Model a stalled task is escalated to. When the loop detector sees the same
    #: structured error recur across a task's recent tool results, the task is
    #: re-queued to run on THIS model with the error history as opening context —
    #: the "route it to a stronger reasoner" step. None keeps the task's own model
    #: and relies on the injected loop-break context alone. Point it at a strong
    #: reasoning model (e.g. a DeepSeek-R1 slug) served by the run's provider.
    escalation_model: str | None = None
    #: --- Agent shell sandbox -------------------------------------------------
    #: An agent's `bash` command is model-authored, i.e. untrusted. These bound
    #: what it can reach and consume; see `worker/sandbox.py` for the layering.
    #: Layer-2 confinement: an argv prefix that wraps every agent shell command
    #: in real mount/network namespaces (bubblewrap, nsjail, systemd-run, a site
    #: helper). `{workspace}` and `{home}` are substituted per command. This is
    #: the ONLY thing that stops one task reading another task's workspace —
    #: same-uid processes can always read each other's files — so configure it on
    #: any multi-tenant or high-fan-out deployment. Empty (the default) leaves
    #: layer 1 only: secrets and resource ceilings are still enforced, filesystem
    #: isolation is not, and the worker logs a warning at boot saying so.
    #: Example: GANTRY_SANDBOX_WRAPPER="bwrap --unshare-all --share-net
    #: --die-with-parent --ro-bind /usr /usr --ro-bind /etc /etc --proc /proc
    #: --dev /dev --bind {workspace} {workspace} --bind {home} {home} --"
    sandbox_wrapper: str | None = None
    #: RLIMIT_AS ceiling per agent shell, in MiB (0 = unlimited). The portable
    #: memory bound without cgroups; keeps a thousand concurrent builds from
    #: OOMing the host. Raise it for runtimes that reserve large sparse address
    #: space (JVM, Go, ASAN builds).
    sandbox_memory_mb: int = 2048
    #: RLIMIT_FSIZE ceiling per agent shell, in MiB (0 = unlimited) — one runaway
    #: log or `yes > file` can otherwise fill the host's disk.
    sandbox_file_size_mb: int = 2048
    #: RLIMIT_NOFILE per agent shell (0 = leave the host default).
    sandbox_open_files: int = 4096
    #: RLIMIT_NPROC per agent shell (0 = disabled, the default). Only meaningful
    #: when each worker runs as its OWN uid: the limit is per-user, so with many
    #: agents sharing one uid a low value throttles every sibling instead of the
    #: offender. Real fork-bomb containment is cgroup pids.max, i.e. the wrapper.
    sandbox_max_processes: int = 0
    #: Extra environment variable names agent shells may inherit. Everything
    #: outside this list plus `sandbox.DEFAULT_ENV_ALLOWLIST` is stripped — that
    #: is what keeps GANTRY_VAULT_KEY, GANTRY_DATABASE_URL and provider API keys
    #: out of untrusted shells. Names that look like credentials are flagged at
    #: boot; add toolchain paths here, never secrets.
    sandbox_env_passthrough: list[str] = Field(default_factory=list)
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
