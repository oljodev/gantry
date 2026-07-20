# Gantry

A stateful multi-agent orchestration and durable execution platform — a
software factory that scales from 2 to 10,000+ concurrent agents running
days-long, goal-directed autonomous coding runs.

**Core idea:** an LLM agent's runtime state is its message history. Gantry
persists every LLM call, tool call, and result to an append-only event log
(`task_events`), which makes crash recovery a replay, human-approval waits
free (parked task rows, zero compute), and the live trace UI a projection
over the same log.

See [docs/MASTERPLAN.md](docs/MASTERPLAN.md) for the full architecture and
phased roadmap.

## Quick start

```sh
./run.sh        # everything: Postgres, migrations, 2 workers, dashboard on :8400
./run.sh dev    # same, plus the Vite dev server with hot reload on :5173
```

No Docker needed — `run.sh` bootstraps Postgres from the pgserver wheel's
bundled binaries into `.gantry/`, generates a `GANTRY_VAULT_KEY` into `.env`
on first run (keep `.env` safe — it now holds secret material), applies
migrations, and starts the full stack. Ctrl-C stops everything.

## GitHub login & private repos (optional)

Sign-in is powered by Supabase GitHub OAuth. One-time setup:

1. In your Supabase project: **Auth → Providers → GitHub → enable**, using a
   GitHub OAuth App whose callback URL is
   `https://<project-ref>.supabase.co/auth/v1/callback`. Add
   `http://localhost:5173` and `http://localhost:8400` as redirect URLs.
2. Backend `.env`: set `GANTRY_SUPABASE_URL` and `GANTRY_ALLOWED_EMAILS`
   (see `.env.example`).
3. Frontend: copy `web/.env.example` to `web/.env.local` with your project
   URL and publishable key (`sb_publishable_...`), then rebuild
   (`rm -rf web/dist && ./run.sh`).

Signing in requests the `repo` scope; the OAuth token is stored AES-256-GCM
encrypted in Gantry's vault and lets workers clone your private repositories.
LLM API keys (OpenAI/Anthropic/Google/OpenRouter/local) are managed the same
way in **Settings → Providers**. With all of it unset, auth is simply off —
ideal for local hacking and CI.

## Development

Requirements: [uv](https://docs.astral.sh/uv/), `make` (Docker optional —
only for the compose workflow).

```sh
make install    # create venv and install dependencies
make dev        # start Postgres 16 (docker compose) — or just use ./run.sh
make migrate    # apply database migrations
make test       # run the test suite
make check      # lint + typecheck + test
make web-check  # frontend typecheck + lint + unit tests
make dev-down   # stop Postgres
```

Configuration is environment-driven (prefix `GANTRY_`); copy `.env.example`
to `.env` for local overrides.

## Layout

```
src/gantry/
  core/      # domain models, task queue, event store, repositories
  runtime/   # agent loop, tool registry, skills loader, compaction
  worker/    # worker entrypoint, sandbox/git integration
  server/    # FastAPI control plane: REST + WebSocket event fanout
  vault/     # encryption, secrets access
migrations/  # Alembic (async)
frontend/    # React observability UI (Phase 5)
skills/      # portable SKILL.md library
infra/       # docker-compose, Dockerfiles
```
