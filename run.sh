#!/usr/bin/env bash
# run.sh — start the full Gantry stack locally with one command. No Docker needed.
#
#   ./run.sh        serve mode: API + workers + built dashboard on http://127.0.0.1:8400
#   ./run.sh dev    dev mode: same, plus the Vite dev server (hot reload) on http://localhost:5173
#
# Postgres runs from the pgserver wheel's bundled binaries in .gantry/ (downloaded
# on first run if no local pg_ctl exists). Ctrl-C stops everything, including Postgres.
set -euo pipefail

MODE="${1:-serve}"
ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

STATE="$ROOT/.gantry"
PGDATA="$STATE/pgdata"
PGSOCK="$STATE/pgsock"
PGPORT="${GANTRY_PGPORT:-54322}"
# Concurrency = number of worker processes; each runs one task at a time.
WORKERS="${GANTRY_WORKERS:-100}"
# Postgres must allow a connection for every worker's LISTEN link plus its small
# session pool (~6/worker, see GANTRY_DB_POOL_SIZE), with headroom for the API
# server, migrations and psql. Sized from the worker count so raising WORKERS
# just works instead of exhausting connections.
PG_MAX_CONN="${GANTRY_PG_MAX_CONN:-$(( WORKERS * 6 + 60 ))}"
mkdir -p "$STATE" "$PGSOCK"

# Fail fast (with a clear message) if the API port is already taken — usually a
# Gantry instance still running in another terminal. Otherwise uvicorn dies with
# a cryptic "address already in use" and the whole script tears itself down.
if (exec 3<>"/dev/tcp/127.0.0.1/8400") 2>/dev/null; then
  exec 3>&- 3<&-
  echo "error: port 8400 is already in use."
  # Name the culprit when we can — a stray background/orphaned server won't be
  # in any visible terminal, so "Ctrl-C it" is useless advice on its own.
  holder=""
  if command -v lsof >/dev/null 2>&1; then
    holder="$(lsof -ti:8400 2>/dev/null | tr '\n' ' ')"
  elif command -v fuser >/dev/null 2>&1; then
    holder="$(fuser 8400/tcp 2>/dev/null | tr -s ' ')"
  fi
  if [ -n "${holder// /}" ]; then
    echo "       Held by PID(s):$holder"
    echo "       Free it with:  kill$holder"
  else
    echo "       Free it with:  fuser -k 8400/tcp"
    echo "                 or:  lsof -ti:8400 | xargs -r kill"
  fi
  echo "       Then re-run ./run.sh"
  exit 1
fi

command -v uv >/dev/null || { echo "error: uv is required — https://docs.astral.sh/uv/"; exit 1; }

# node/npm are needed to build the dashboard. nvm shells don't export them to
# non-interactive runs, so locate them ourselves.
if ! command -v npm >/dev/null 2>&1; then
  if [ -s "$HOME/.nvm/nvm.sh" ]; then
    export NVM_DIR="$HOME/.nvm"
    # shellcheck disable=SC1091
    . "$NVM_DIR/nvm.sh" >/dev/null 2>&1 || true
  fi
  if ! command -v npm >/dev/null 2>&1; then
    node_bin="$(ls -d "$HOME"/.nvm/versions/node/*/bin 2>/dev/null | sort -V | tail -1)"
    [ -n "$node_bin" ] && PATH="$node_bin:$PATH"
  fi
fi

echo "==> syncing python deps"
uv sync --group dev --quiet

# --- vault key (encrypts API keys / GitHub token at rest) ----------------
if ! grep -qs '^GANTRY_VAULT_KEY=' .env; then
  echo "==> generating GANTRY_VAULT_KEY into .env (keep .env safe — it now holds secret material)"
  printf 'GANTRY_VAULT_KEY=%s\n' "$(uv run python -c 'import secrets; print(secrets.token_hex(32))')" >> .env
fi

# --- postgres binaries ---------------------------------------------------
PGBIN=""
if [ -x "$STATE/pgwheel/pgserver/pginstall/bin/pg_ctl" ]; then
  PGBIN="$STATE/pgwheel/pgserver/pginstall/bin"
elif command -v pg_ctl >/dev/null && command -v initdb >/dev/null; then
  PGBIN="$(dirname "$(command -v pg_ctl)")"
else
  echo "==> downloading bundled Postgres (pgserver wheel, one-time)"
  python3 -m pip download pgserver --no-deps --only-binary=:all: \
    --python-version 312 --implementation cp -d "$STATE/wheels" --quiet
  python3 - "$STATE" <<'PY'
import glob, sys, zipfile
state = sys.argv[1]
wheel = sorted(glob.glob(f"{state}/wheels/pgserver-*.whl"))[-1]
zipfile.ZipFile(wheel).extractall(f"{state}/pgwheel")
PY
  chmod -R u+x "$STATE/pgwheel/pgserver/pginstall/bin"
  PGBIN="$STATE/pgwheel/pgserver/pginstall/bin"
fi

# --- init + start postgres ----------------------------------------------
if [ ! -f "$PGDATA/PG_VERSION" ]; then
  echo "==> initdb $PGDATA"
  "$PGBIN/initdb" -D "$PGDATA" -U gantry --auth=trust -E UTF8 >/dev/null
fi

PG_OPTS="-p $PGPORT -k $PGSOCK -c listen_addresses=127.0.0.1 -c max_connections=$PG_MAX_CONN"
if ! "$PGBIN/pg_ctl" -D "$PGDATA" status >/dev/null 2>&1; then
  echo "==> starting postgres on 127.0.0.1:$PGPORT (max_connections=$PG_MAX_CONN)"
  "$PGBIN/pg_ctl" -D "$PGDATA" -w -l "$STATE/postgres.log" -o "$PG_OPTS" start >/dev/null
  STARTED_PG=1
else
  STARTED_PG=0
  # A server left running from a smaller WORKERS can't fit the connections we
  # now need (max_connections only changes on restart) — restart it to resize.
  current="$("$PGBIN/psql" -h 127.0.0.1 -p "$PGPORT" -U gantry -d postgres -tAc \
    'SHOW max_connections' 2>/dev/null | tr -d '[:space:]')"
  if [ -n "$current" ] && [ "$current" -lt "$PG_MAX_CONN" ]; then
    echo "==> restarting postgres to raise max_connections $current -> $PG_MAX_CONN"
    "$PGBIN/pg_ctl" -D "$PGDATA" -w -l "$STATE/postgres.log" -o "$PG_OPTS" restart >/dev/null
    STARTED_PG=1
  fi
fi

if ! "$PGBIN/psql" -h 127.0.0.1 -p "$PGPORT" -U gantry -d postgres -tAc \
    "SELECT 1 FROM pg_database WHERE datname='gantry'" 2>/dev/null | grep -q 1; then
  echo "==> creating database 'gantry'"
  "$PGBIN/createdb" -h 127.0.0.1 -p "$PGPORT" -U gantry gantry
fi

export GANTRY_DATABASE_URL="postgresql+asyncpg://gantry@127.0.0.1:$PGPORT/gantry"

echo "==> applying migrations"
uv run alembic upgrade head

# --- frontend ------------------------------------------------------------
# Rebuild when the bundle is missing OR any source file is newer than it —
# otherwise `git pull` + ./run.sh silently serves a stale dashboard and none of
# the latest UI changes appear.
if [ "$MODE" = "serve" ]; then
  needs_build=0
  if [ ! -f web/dist/index.html ]; then
    needs_build=1
  elif [ -n "$(find web/src web/index.html web/package.json web/vite.config.* \
      -newer web/dist/index.html 2>/dev/null | head -1)" ]; then
    needs_build=1
    echo "==> dashboard sources changed since last build"
  fi
  if [ "$needs_build" = 1 ]; then
    echo "==> building dashboard"
    (cd web && npm install && npm run build)
  fi
fi
if [ "$MODE" = "dev" ] && [ ! -d web/node_modules ]; then
  (cd web && npm install)
fi

# --- processes -----------------------------------------------------------
PIDS=()
cleanup() {
  trap - EXIT INT TERM
  echo
  echo "==> shutting down"
  for pid in "${PIDS[@]:-}"; do kill "$pid" 2>/dev/null || true; done
  wait 2>/dev/null || true
  if [ "$STARTED_PG" = 1 ]; then
    "$PGBIN/pg_ctl" -D "$PGDATA" -m fast stop >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT INT TERM

echo "==> starting $WORKERS worker(s)"
for _ in $(seq 1 "$WORKERS"); do
  uv run python -m gantry.worker &
  PIDS+=($!)
done

if [ "$MODE" = "dev" ]; then
  echo "==> starting vite dev server on http://localhost:5173"
  (cd web && exec node node_modules/vite/bin/vite.js) &
  PIDS+=($!)
fi

echo
echo "    Gantry dashboard:  http://127.0.0.1:8400"
[ "$MODE" = "dev" ] && echo "    Vite (hot reload): http://localhost:5173"
echo

# --no-access-log: WS auth tokens ride the query string; keep them out of logs.
exec_uvicorn() {
  uv run uvicorn --factory gantry.server.app:create_app \
    --host 127.0.0.1 --port 8400 --no-access-log
}
exec_uvicorn
