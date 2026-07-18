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

## Development

Requirements: [uv](https://docs.astral.sh/uv/), Docker (for Postgres), `make`.

```sh
make install    # create venv and install dependencies
make dev        # start Postgres 16 (docker compose)
make migrate    # apply database migrations
make test       # run the test suite
make check      # lint + typecheck + test
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
