COMPOSE := docker compose -f infra/docker-compose.yml

.PHONY: install dev dev-down dev-logs migrate revision server worker workers-up test lint fmt typecheck check web-install web-dev web-build web-check

install:
	uv sync

dev:
	$(COMPOSE) up -d --wait

dev-down:
	$(COMPOSE) down

dev-logs:
	$(COMPOSE) logs -f

migrate:
	uv run alembic upgrade head

revision:
	uv run alembic revision --autogenerate -m "$(m)"

server:
	uv run uvicorn --factory gantry.server.app:create_app --reload

worker:
	uv run python -m gantry.worker

workers-up:  # N containerized workers: make workers-up n=4
	$(COMPOSE) --profile workers up -d --build --scale worker=$(or $(n),2)

test:
	uv run pytest

lint:
	uv run ruff check src tests migrations
	uv run ruff format --check src tests migrations

fmt:
	uv run ruff check --fix src tests migrations
	uv run ruff format src tests migrations

typecheck:
	uv run mypy

check: lint typecheck test

# --- Frontend -----------------------------------------------------------

web-install:
	cd web && npm install

web-dev:  # Vite dev server (proxies /api + WS to the control plane)
	cd web && npm run dev

web-build:  # production bundle; FastAPI serves web/dist automatically
	cd web && npm run build

web-check:  # typecheck + lint + unit tests
	cd web && npm run check
