# Gantry — notes for Claude

## Local dev

`./run.sh` starts the whole stack (bundled Postgres via the pgserver wheel,
migrations, one async worker process driving many agents concurrently —
`GANTRY_WORKER_CONCURRENCY`, default 100 — dashboard on `:8400`). `./run.sh dev`
adds the Vite dev server on `:5173`. No Docker. Gate: `make check` (ruff, mypy
strict, pytest) and `cd web && npm run check` (tsc, oxlint, vitest).

The worker is a single asyncio process: a dispatcher claims tasks and runs up to
`worker_concurrency` agent loops at once, all sharing one lean DB pool
(`db_pool_size`/`db_max_overflow`) and outbound LLM pacers (`llm_max_rps`/
`llm_rps_burst`, a non-blocking GCRA limiter **per provider base_url**, so a
throttled provider never stalls another). Agents spend ~all their time awaiting
network I/O, so one process replaces the old OS-process-per-worker pool.

**Scale one process** (raise `worker_concurrency` + `llm_max_rps`), not many
processes: each worker PROCESS has its OWN pacers and DB pool, so `GANTRY_WORKERS`
> 1 silently multiplies both the global rps (`N x llm_max_rps`) and DB
connections. Budget `(worker + API pools) x GANTRY_WORKERS` well under pgserver's
`max_connections` (~100, minus reserved/NOTIFY/reaper); the API can size its own
pool via `api_db_pool_size`/`api_db_max_overflow`. `GANTRY_WORKERS` stays 1 by
default and is only for deliberately wanting that multiplication.

## Safety invariants (do not regress these)

- **Agent shells are confined** (`worker/sandbox.py`). Every `bash` command runs
  with an allowlisted environment (no vault key, DB URL, or provider key ever
  reaches it), rlimit ceilings, a private `HOME`/`TMPDIR`, and its **own process
  group** so a timeout or a cancel kills the whole tree. Filesystem isolation
  *between* tasks needs `GANTRY_SANDBOX_WRAPPER` (bubblewrap/nsjail) — without it
  same-uid tasks can read each other's workspaces, and the worker says so at boot.
  If you add a way to run a subprocess for an agent, route it through `sandbox`.
- **Tool errors are structured, not prose.** `runtime/diagnostics.py` parses
  compiler/test output into `(file, line, column, code, message)` with a stable
  fingerprint. Two things depend on it: the repair-loop breaker (the same error
  recurring across a task's recent results stalls the task and escalates it to
  `escalation_model` via a `task_escalated` EVENT — never a payload edit), and
  prompt pruning (a failing build reaches the model as a short error list; the
  full log still streams to `terminal_chunk`). Parsers are rigid regexes on
  purpose — one that guesses would invent or mask loops. A live fold into
  `AgentState` must mirror the `rehydrate` fold exactly, or a resumed task
  disagrees with a live one about whether it is stuck.
- **Death-loop circuit breakers make a runaway impossible by invariant**, each a
  pure function of durable state (so a resume decides identically): per-agent
  step caps are **always finite** and role-aware — a delegating agent
  (`leader_max_steps`, 150) halts *gracefully* with a report instead of looping
  forever, a spawned non-interactive micro-task (`execute_max_steps`, 25)
  *fails* fast so its leader learns it's stuck, a standalone task keeps
  `default_max_steps`. And the spawn tools refuse to fan out once a leader has
  too many terminally-FAILED direct children (`max_repair_failures`, the
  repair-wave brake) or the run's whole tree hits `run_task_ceiling` — so a
  leader can't fund fixer wave after fixer wave.

- **Attachments are routed by model capability, never assumed.**
  `attachments/capabilities.py` maps a model slug to the modalities it can
  ingest, and an unknown slug is **text-only on purpose**: under-claiming costs
  one cheap transcription, over-claiming is a hard provider 400 mid-run. Before
  the loop starts, `attachments/prepare.py` resolves every attached file into a
  form THIS task's model can read — an image block for a vision model, a
  vision-model transcription for a text-only worker (DeepSeek-R1, Qwen Coder),
  an explicit note when neither is possible (never a silent omission). The
  transcript is cached on the attachment row, so a resume reuses the identical
  text instead of paying for a second, subtly different description. Uploads
  sniff their media type from the BYTES (never the client's Content-Type), and
  blob keys are content-addressed so a user filename never reaches the
  filesystem. Attachments fold into the **goal message**, which is a compaction
  anchor — a spec survives the whole run.

- **Every LLM call is metered, and the ledger must reconcile.** The billing hook
  is a *client decorator* (`billing/metering.py`), not a provider callback, so
  agent steps, the compaction summarizer, the conflict resolver and the vision
  pre-pass are all billed by construction — a new call site cannot spend
  off-ledger. Each response writes one `llm_usage_logs` row **and** decrements
  `users.gantry_credits_balance` in ONE transaction, via a single
  `UPDATE ... balance = balance - :c` (never read-modify-write, or a swarm of
  agents on one account would interleave and lose charges). The invariant is
  `balance == start - SUM(credits_deducted)`. Charges are priced from the
  *requested* slug, never the provider-echoed `response.model`. A balance may go
  negative — the provider already billed us, so hiding the charge would not
  un-spend it; enforcement (`enforce_credit_balance`) gates *before* a task
  starts. Billing failures log at ERROR and never fail the agent's turn.
  Pricing is `raw_cost / credit_cost_ratio x credits_per_usd`, in Decimal
  throughout: `credit_cost_ratio` is the COST ratio, so 0.60 means a **40% gross
  margin**. Runs are attributed at launch and children inherit `user_id` through
  `queue.enqueue`, so a whole swarm bills one account.

- **Running out of credit PAUSES a run; it never fails it.** The gate is checked
  at a step boundary (the worker's `on_step`), so an in-flight tool call always
  finishes and the run stops *between* actions — a half-applied edit is
  unrecoverable. The task parks as `PAUSED_OUT_OF_CREDITS`, which is a parked
  status like `waiting_approval`: unleased, log intact, resumed by
  `queue.resume_paused_*` to PENDING with `max_attempts` lifted (a pause must not
  erode the error-retry budget). `park_for_credits` re-reads the balance inside
  the park transaction, so a top-up racing the park can't strand a funded run.
  The affordability check reads a `CreditGate` fed by the metering path's
  `UPDATE ... RETURNING`, NOT the database — a query per step would be ~15k
  queries for one swarm. Because the next call's cost is unknowable, an account
  overruns its last credit by at most one call. A git-backed task pushes its WIP
  to its branch before parking, since the workspace is per-attempt and a resume
  re-clones. **Granting credits is creating money**: `/api/credits/grant` sits
  outside the blanket bearer dependency (its caller is a webhook with no session)
  and `authorize_grant` is the only gate — internal secret via
  `hmac.compare_digest`, an `admin_emails` account, or auth-disabled dev. An
  ordinary authenticated user is always 403.

- **Every string that reaches a TEXT/JSONB column goes through
  `core/sanitize.py` first.** Postgres cannot store a NUL byte (`\x00`) in
  either column type — not even escaped — and one is trivially reachable: a
  `bash` tool call is model-authored (`cat` a binary file, `git diff` a binary
  blob), its output is decoded with `errors="replace"`, which passes a literal
  NUL through unchanged, and that string is headed straight for a
  `terminal_chunk` event. Rather than patch every producer, `append_event`
  (every event payload), `queue.complete`/`fail` (`Task.result`/`last_error`),
  and the attachment text/transcript paths all sanitize at the write boundary —
  a producer fixed later, or a new one that forgets, is still safe. Only NUL is
  touched; this must never grow into a general control-character filter, since
  the terminal pane renders real ANSI colour codes.

- **The Paddle webhook authenticates itself; nothing else does.**
  `POST /api/webhooks/paddle` sits outside every router's auth dependency (a
  payment webhook cannot present our bearer token or admin secret) and instead
  verifies Paddle's own HMAC-SHA256 `Paddle-Signature` header
  (`billing/paddle.py`) against `raw_body` — read via `request.body()` BEFORE
  anything touches `request.json()`, since the signature covers the exact bytes
  Paddle sent. An unconfigured `paddle_webhook_secret` fails CLOSED (401), never
  "accept unsigned". The signed timestamp must additionally be within
  `paddle_signature_tolerance_seconds` of now, so a captured valid signature
  can't be replayed later. Every delivery inserts its Paddle `event_id` into
  `paddle_webhook_events` (PK) and grants credits in ONE transaction via the
  shared `billing.ledger.grant_credits_and_resume` — a retry-redelivered event
  (Paddle retries on anything but a prompt 2xx) collides on the PK and is
  skipped, so a redelivery can't double-grant. The only link between a Paddle
  purchase and a Gantry account is `customData.user_id`, set client-side at
  `Paddle.Checkout.open()` and echoed back on the webhook — the key name must
  match exactly on both sides (`lib/paddle.ts` / `billing/paddle.py`). A webhook
  shaped in a way we didn't anticipate (unmapped price, non-USD total, unknown
  user_id) degrades to `"skipped"` (still 200), never a crash — Paddle retries
  4xx/5xx responses, so raising on a parsing surprise would crash-loop it.

Per-task tuning is role-aware: a leaf EXECUTE worker compacts at a tighter budget
(`execute_max_context_tokens`) so a small task never compacts and its prompt cache
stays warm; a delegating leader gets a larger one (`leader_max_context_tokens`).
When a run's root task settles, the worker logs a `worker.run_rollup` line (spend,
cache-hit ratio, compactions, per-status counts) for the whole tree.

## Deployment (IMPORTANT)

**`gantry.oljo.dev` is a Cloudflare Pages site that auto-deploys the frontend
from the GitHub `main` branch.** It serves the built SPA (`web/dist`) and
nothing else. **To change what's live, commit and push to `main`** — there is
no separate deploy step; Cloudflare rebuilds on push.

Cloudflare Pages build settings: root directory `web`, build command
`npm run build`, output `web/dist`. Build-time env vars live in
**Pages → Settings → Environment variables** (see below), not in the repo.

### The backend does NOT run on Cloudflare

Gantry's backend is a long-running Python/FastAPI app with Postgres
`LISTEN/NOTIFY`, persistent worker loops, git, and sandboxes. **None of that
fits Cloudflare Workers/Pages** (no long-running processes, no Postgres, CPU
limits). Cloudflare hosts the *frontend only*. The backend must run on a real
server/VM/container host, and the deployed frontend talks to it over the
network.

If the deployed site shows the amber "Backend not reachable" banner (or, before
this was handled, `SyntaxError: Unexpected token '<'`), it means `/api/*` is
hitting the static host and getting `index.html` back — i.e. no backend is
reachable. That is expected until a backend is hosted and wired up.

### Wiring the frontend to a backend

The frontend calls the API at `VITE_API_BASE` (build-time env). Blank =
same-origin (local dev / FastAPI-served SPA). For the split Cloudflare deploy,
set `VITE_API_BASE` to the backend's public origin; WebSockets follow the same
host automatically. See `web/.env.example`.

Cleanest path given we're already on Cloudflare — **expose a self-hosted backend
via a Cloudflare Tunnel**:

1. Run the backend on a machine that stays up: `./run.sh` (or just the server +
   workers against a persistent Postgres).
2. `cloudflared tunnel` mapping e.g. `https://api.gantry.oljo.dev` → the local
   `:8400`.
3. In Cloudflare Pages env vars set `VITE_API_BASE=https://api.gantry.oljo.dev`
   (plus `VITE_SUPABASE_URL` / `VITE_SUPABASE_KEY` for login), then push to
   redeploy.
4. Backend env: set `GANTRY_CORS_ORIGINS` to include `https://gantry.oljo.dev`
   (the prod origin is in the default list), and `GANTRY_SUPABASE_URL` /
   `GANTRY_ALLOWED_EMAILS` to enable auth.

Any host that runs a container works instead of a tunnel (Fly.io, Railway, a
VPS) — point `VITE_API_BASE` at it and add its origin to CORS.

## Auth / GitHub login prerequisites (manual, one-time)

Login is Supabase GitHub OAuth (project `kacjngjvcalwnukyreov`). It stays OFF
until configured:
- Register a GitHub OAuth App; callback
  `https://kacjngjvcalwnukyreov.supabase.co/auth/v1/callback`.
- Supabase → Authentication → Providers → GitHub → enable with the app's client
  id/secret; add the site origin(s) under Redirect URLs.
- Frontend build env: `VITE_SUPABASE_URL`, `VITE_SUPABASE_KEY`
  (`sb_publishable_...`). Backend env: `GANTRY_SUPABASE_URL`,
  `GANTRY_ALLOWED_EMAILS`.

## Conventions

- **Never use emojis.** Not in the UI, not in code, comments, docs, commit
  messages, test names, log output, or CLI scripts. In the frontend, every icon
  is a `lucide-react` component (`<Check className="h-4 w-4" aria-hidden />`) —
  never an emoji and never a decorative Unicode glyph (`✓ ✗ ⚠ ▾ ● ☰ ⌬ ⧉`) used
  as a stand-in for one. Prose should carry meaning on its own; if a marker is
  genuinely needed outside the UI, use a plain word.
- The UI is written **dark-first**: light mode works by inverting Tailwind's
  colour ramps in `web/src/index.css` (`[data-theme="light"]`), not by
  annotating components with `dark:`. So keep using dark-first shades — light
  text = low number (`text-zinc-200`), dark surface = high number
  (`bg-zinc-950`) — and light mode follows automatically. Code/terminal panes
  use the `bg-code` token rather than `bg-black`.
- Secrets (LLM keys, GitHub token) live only as AES-256-GCM ciphertext in the
  `secrets` table; `GANTRY_VAULT_KEY` (32-byte hex) is generated into `.env` by
  `run.sh`. `.env` is secret material.
- Every DB row is workspace-scoped (`DEFAULT_WORKSPACE_ID` for now).
- Task payloads carry `provider_id` (never keys); workers decrypt at claim time.
- Teams snapshot their whole tree into the launch payload — never re-read
  profiles mid-run.
- `git push` / deploy only when the user asks (they did, for this site).
- **Commits: always split a batch of work into several reasonably-sized,
  coherent commits** — grouped by feature or area — rather than one large
  catch-all commit. Do this every time, even when the user just says "commit
  everything." Order them so the tree stays buildable, and keep each commit's
  message focused on its own change.
