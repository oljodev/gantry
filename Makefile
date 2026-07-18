COMPOSE := docker compose -f infra/docker-compose.yml

.PHONY: install dev dev-down dev-logs migrate revision server test lint fmt typecheck check

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
