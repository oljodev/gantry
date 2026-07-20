# Gantry — notes for Claude

## Local dev

`./run.sh` starts the whole stack (bundled Postgres via the pgserver wheel,
migrations, 2 workers, dashboard on `:8400`). `./run.sh dev` adds the Vite dev
server on `:5173`. No Docker. Gate: `make check` (ruff, mypy strict, pytest) and
`cd web && npm run check` (tsc, oxlint, vitest).

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

- Secrets (LLM keys, GitHub token) live only as AES-256-GCM ciphertext in the
  `secrets` table; `GANTRY_VAULT_KEY` (32-byte hex) is generated into `.env` by
  `run.sh`. `.env` is secret material.
- Every DB row is workspace-scoped (`DEFAULT_WORKSPACE_ID` for now).
- Task payloads carry `provider_id` (never keys); workers decrypt at claim time.
- Teams snapshot their whole tree into the launch payload — never re-read
  profiles mid-run.
- `git push` / deploy only when the user asks (they did, for this site).
