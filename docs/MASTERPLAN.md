# Gantry — Master Architecture & Development Roadmap

> A stateful multi-agent orchestration and durable execution platform: a software
> factory that scales from 2 to 10,000+ concurrent agents running days-long,
> goal-directed autonomous coding runs.

**Status:** Founding document. Decisions recorded here were made 2026-07-18.

---

## 1. Locked-in decisions

| Decision | Choice | Rationale |
|---|---|---|
| Agent runtime | Custom loop over **LiteLLM** (provider-agnostic) | Multi-model freedom (Claude, GPT, local). Owning the loop is required anyway for per-step durable checkpointing — the loop *is* the event log. |
| MVP scope | **Single-tenant depth** | Prove planner→workers→checkpoint/resume→HITL→trace UI end-to-end. Every table carries `workspace_id` from day 1; enforcement (auth/RLS) lands later. |
| Database | **Postgres 16 in Docker**, cloud-agnostic | Full control of `FOR UPDATE SKIP LOCKED`, `LISTEN/NOTIFY`, extensions, pooling. Deployable later to RDS/Cloud SQL/Supabase/self-hosted. |
| Git target | **Real GitHub early** | Workers clone/branch/commit/push real repos via fine-grained PAT from the first integration phase. |

## 2. Architectural analysis

### 2.1 The core insight: the event log IS the agent state

An LLM agent's runtime state is its message history. If every LLM
request/response, tool call, and tool result is appended to `task_events`
*before and after* it happens, then:

- **Crash recovery is replay.** A worker that dies mid-task loses nothing; any
  other worker rehydrates the message history from events and continues from
  the exact step.
- **HITL dormancy is free.** "Sleeping" on an approval gate is not a blocked
  process — the task is *parked* (status `waiting_approval`), the worker moves
  on to other work, and 0% compute is consumed. Approval appends an event and
  re-queues the task; whoever claims it resumes from the log.
- **The trace UI is a projection.** The trace tree, terminal streams, and diffs
  are all read models over `task_events`. One write path, many views.

This is why we build the loop ourselves rather than embedding a black-box agent
framework: durability demands checkpoint hooks at every step.

### 2.2 Queue correctness at scale

- **Claiming:** single-statement `UPDATE … FROM (SELECT … FOR UPDATE SKIP
  LOCKED LIMIT n)` — thousands of stateless workers poll without collisions or
  serialization errors.
- **Leases, not locks:** holding a row lock for a task's lifetime would pin a
  transaction open for hours. Instead the claim sets `claimed_by` +
  `lease_expires_at`; workers heartbeat to extend the lease; a **reaper**
  re-queues tasks whose lease expired (worker died). Combined with event-log
  resume, a re-queued task loses only the in-flight step.
- **Wakeups:** `LISTEN/NOTIFY` on enqueue/approval nudges idle workers
  instantly; jittered polling remains as the reliable fallback (NOTIFY is
  best-effort and doesn't survive disconnects).
- **Idempotency:** a `tool_call` event is written *before* execution and its
  `tool_result` after. On resume with a dangling `tool_call`, per-tool policy
  decides: re-run (idempotent: read, ls, test run) or verify-then-skip
  (non-idempotent: `git push`, file write with content hash).
- **10k-worker realities (later phases):** PgBouncer transaction pooling,
  `task_events` partitioning + archival, batched event writes, and moving
  terminal-chunk firehose off the durable path if needed.

### 2.3 Orchestration model (Planner–Worker)

Cursor-style: a **planner task** (long-lived agent) decomposes a goal into
child tasks via a `spawn_subtask` tool, then parks in `waiting_children`
(again: 0 compute) until children finish; completion events wake it to
integrate results, spawn follow-ups, or finish. Tasks form a tree
(`parent_task_id`, `root_task_id`), which is exactly the trace tree the UI
renders. Depth is unbounded — workers can themselves plan.

### 2.4 Sandboxing & git

Each worker is a Docker container with an ephemeral workspace volume. Tools:
file read/edit, bash (with per-command timeout + output capture as
`terminal_chunk` events), and git operations (clone with token-injected remote,
feature branch per task, commit, push). Tests run inside the sandbox. Network
egress lockdown and resource quotas are hardening-phase work.

### 2.5 Skills

`skills/<name>/SKILL.md` — Markdown with YAML frontmatter (name, description,
when-to-use). Injected into a worker's system prompt only when the task's
payload references them (or a lightweight matcher selects them). Keeps the base
prompt small; skills are portable text, versioned in git.

### 2.6 Multi-tenancy & vault (schema-ready now, enforced later)

- `workspace_id UUID NOT NULL` on every table from the first migration; all
  queries scoped through a repository layer so RLS can be enabled later without
  rewrites.
- **Vault:** `secrets` table, AES-256-GCM envelope encryption — per-workspace
  data keys wrapped by a master key (env var locally, KMS in cloud). GitHub
  PATs and LLM API keys never appear in plaintext at rest or in events/logs
  (redaction middleware).

## 3. System components

```
┌────────────┐   REST/WS    ┌──────────────┐
│  React UI  │◄────────────►│   FastAPI     │──── LISTEN/NOTIFY ────┐
│ (Vite+TW)  │              │ control plane │                       │
└────────────┘              └──────┬───────┘                        ▼
                                   │                     ┌──────────────────┐
                                   ▼                     │   PostgreSQL 16   │
                            enqueue/approve/read         │ tasks, task_events│
                                                         │ workspaces,secrets│
┌─────────────────────────────┐   claim/heartbeat/append │      (Alembic)    │
│ Worker fleet (Docker, N×)   │◄────────────────────────►└──────────────────┘
│ agent loop (LiteLLM) + tools│
│ git sandbox workspace       │────── clone/push ──────► GitHub
└─────────────────────────────┘
```

## 4. Data model (core tables)

**`tasks`** — unit of work: `id` (UUIDv7), `workspace_id`, `parent_task_id`,
`root_task_id`, `kind` (plan | execute), `status` (pending, claimed, running,
waiting_approval, waiting_children, succeeded, failed, cancelled), `priority`,
`payload` JSONB (goal, repo, base branch, skills, model config), `result`
JSONB, `attempt`/`max_attempts`, `claimed_by`, `lease_expires_at`,
`scheduled_at`, timestamps. Partial index on `(priority DESC, created_at)
WHERE status = 'pending'` for O(1) claims.

**`task_events`** — append-only log: `id` BIGSERIAL, `task_id`, `seq`
(per-task monotonic, `UNIQUE(task_id, seq)`), `event_type` (llm_request,
llm_response, tool_call, tool_result, terminal_chunk, compaction,
approval_requested, approval_resolved, status_change, error, checkpoint),
`payload` JSONB, `created_at`. Designed for range partitioning later.

**`workspaces`, `users`, `secrets`** — tenancy + vault (minimal rows in MVP,
full enforcement in Phase 9).

## 5. Tech stack

Backend: Python 3.12, uv, FastAPI, SQLAlchemy 2 (async) + asyncpg, Alembic,
Pydantic v2, LiteLLM, structlog. Testing: pytest + pytest-asyncio +
testcontainers (real Postgres in tests — queue concurrency cannot be tested
against SQLite). Frontend: Vite + React 18 + TypeScript + Tailwind + TanStack
Query, native WebSocket. Infra: docker-compose (dev), Dockerfiles per service.

Repo layout (single package, module-per-concern; split into packages only if
needed):

```
gantry/
  pyproject.toml
  src/gantry/
    core/        # domain models, task queue, event store, repositories
    runtime/     # agent loop, tool registry, skills loader, compaction
    worker/      # worker entrypoint, sandbox/git integration
    server/      # FastAPI app: REST + WebSocket, event fanout
    vault/       # encryption, secrets access
  migrations/    # Alembic
  frontend/      # React app
  skills/        # SKILL.md library
  infra/         # docker-compose.yml, Dockerfiles
  docs/
  tests/
```

## 6. Roadmap

Each phase ends **green and demonstrable**. MVP = end of Phase 8.

### Phase 0 — Scaffolding & foundations
Repo layout above; uv + ruff + mypy + pytest wiring; docker-compose with
Postgres 16; Alembic baseline; typed settings (pydantic-settings); structlog;
CI skeleton (GitHub Actions: lint + test). **Demo:** `make dev` boots PG,
`make test` is green.

### Phase 1 — Durable queue core ⭐ (the foundation everything rests on)
`tasks` + `task_events` migrations; SKIP LOCKED claim; lease heartbeat; reaper;
event append with per-task `seq`; LISTEN/NOTIFY wakeups; retry/backoff
semantics. **Demo/proof:** a brutal concurrency test — 50 fake workers hammer
1,000 tasks; assert zero double-claims, zero lost tasks, kill -9'd workers'
tasks re-queued by the reaper.

### Phase 2 — Agent runtime (durable loop)
LiteLLM chat loop with streaming; tool registry + JSON-schema dispatch;
**checkpoint-per-step**: every request/response/tool event appended before/after
execution; resume-from-log rehydration; dangling-tool-call policy; context
compaction (summarize + `compaction` event when nearing the window).
**Demo/proof:** an agent solving a multi-step task is `kill -9`'d mid-run and a
fresh process finishes it correctly from the log.

### Phase 3 — Sandboxed workers & git
Worker Docker image; workspace lifecycle (create/mount/destroy); git tools:
clone (PAT-injected remote), branch-per-task, add/commit/push; bash tool with
timeouts and `terminal_chunk` capture; test execution. **Demo:** a worker
clones a real GitHub repo, makes an edit, runs tests, pushes
`gantry/task-<id>`.

### Phase 4 — Control plane API + realtime
FastAPI: enqueue runs, inspect tasks/trees, read event streams; WebSocket
fanout bridging `task_events` (via NOTIFY + tail-follow) to subscribers; typed
API client. **Demo:** `curl` enqueues a run; `wscat` watches live events.

### Phase 5 — Observability frontend
React app: run dashboard, **trace tree** (task hierarchy + event timeline),
live **terminal streaming**, **diff viewer** (per-task branch vs base).
**Demo:** watch a single agent work in real time in the browser. (UI lands
*before* multi-agent orchestration because debugging a fleet without eyes is
misery.)

### Phase 6 — Planner–Worker orchestration
Planner agent kind; `spawn_subtask` tool; `waiting_children` parking + wakeup
on child completion; result aggregation; failure policy (child fails →
planner decides: retry, respawn differently, or abort); run-level concurrency
caps. **Demo:** goal in → plan → 5+ parallel workers → integrated result → one
pushed branch, whole tree live in the UI.

### Phase 7 — Human-in-the-loop gates
Tool-level policy engine (destructive commands ⇒ gate); `approval_requested`
parking; approve/reject API + UI inbox with diff/command preview; WS push
notification. **Demo:** worker attempts `rm -rf` / force-push, parks at 0%
compute; human approves in UI; task resumes seamlessly.

### Phase 8 — Skills system → **🏁 Local MVP**
SKILL.md format + frontmatter; skills registry; injection by task payload +
matcher; skill usage visible in traces. **MVP acceptance:** *"Given a GitHub
repo and a goal, Gantry plans, fans out sandboxed workers, survives worker
kills, pauses for approval on destructive actions, and delivers pushed feature
branches — all observable live in the browser."*

### Phase 9 — Multi-tenancy enforcement & vault
Auth (session/JWT), workspace membership, repository-layer scoping everywhere
+ Postgres RLS as defense-in-depth; AES-256-GCM vault with envelope
encryption; secret redaction in events/logs; per-workspace GitHub PATs and LLM
keys.

### Phase 10 — Scale & reliability hardening
PgBouncer; `task_events` range partitioning + archival to object storage;
batched event writes; backpressure and run-level rate/spend budgets;
Prometheus metrics + OpenTelemetry traces; load test: 1k+ concurrent
simulated workers with p99 claim latency targets; chaos suite (kill workers,
kill PG failover, network partitions).

### Phase 11 — Cloud SaaS deployment
Helm chart / K8s manifests (or ECS); worker autoscaling on queue depth
(KEDA-style); managed Postgres; KMS-backed master key; container registry +
CD; TLS/ingress; billing/usage metering hooks; egress-restricted sandbox
profiles.

## 7. Cross-cutting principles

1. **Every phase merges green** — concurrency code without tests is fiction.
2. **The event log is sacred** — append-only, ordered, redacted, never mutated.
3. **Workers are cattle** — any worker can die at any moment; correctness must
   not depend on graceful shutdown.
4. **Compute-free waiting** — no process ever blocks on a human or a child
   task; parked tasks cost nothing.
5. **Tenancy-shaped from day 1** — `workspace_id` everywhere, even while
   enforcement waits for Phase 9.
