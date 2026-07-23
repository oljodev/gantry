"""The /api/usage projection over the event log: totals, daily, by-model."""

from __future__ import annotations

import uuid

import httpx
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.events import append_event
from gantry.core.models import DEFAULT_WORKSPACE_ID, EventType, Task, TaskKind

Sessions = async_sessionmaker[AsyncSession]


def _usage(prompt: int, completion: int, read: int = 0, write: int = 0) -> dict[str, int]:
    return {
        "prompt_tokens": prompt,
        "completion_tokens": completion,
        "cache_read_tokens": read,
        "cache_write_tokens": write,
    }


async def _seed(db: Sessions) -> Task:
    async with session_scope(db) as session:
        task = await queue.enqueue(
            session,
            workspace_id=DEFAULT_WORKSPACE_ID,
            kind=TaskKind.EXECUTE,
            payload={"goal": "do a thing"},
        )
        await append_event(
            session,
            task.id,
            EventType.LLM_RESPONSE,
            {"model": "claude-opus-4-8", "usage": _usage(100, 20, read=80, write=10)},
        )
        await append_event(
            session,
            task.id,
            EventType.LLM_RESPONSE,
            {"model": "claude-opus-4-8", "usage": _usage(50, 5, read=40)},
        )
        # A compaction call carries usage but no model -> the "summarizer" bucket.
        await append_event(
            session,
            task.id,
            EventType.COMPACTION,
            {"summary": "s", "kept_seqs": [], "usage": _usage(200, 30)},
        )
        # A non-usage event must never be counted.
        await append_event(session, task.id, EventType.LLM_REQUEST, {"step": 1})
    return task


async def test_usage_totals_sum_across_events(client: httpx.AsyncClient, db: Sessions) -> None:
    await _seed(db)
    body = (await client.get("/api/usage")).json()

    assert body["prompt_tokens"] == 350  # 100 + 50 + 200 (compaction included)
    assert body["completion_tokens"] == 55  # 20 + 5 + 30
    assert body["cache_read_tokens"] == 120  # 80 + 40
    assert body["cache_write_tokens"] == 10
    assert body["llm_calls"] == 2  # compaction is not a "call"


async def test_usage_by_model_splits_summarizer(client: httpx.AsyncClient, db: Sessions) -> None:
    await _seed(db)
    by_model = {m["model"]: m for m in (await client.get("/api/usage")).json()["by_model"]}

    assert by_model["claude-opus-4-8"]["prompt_tokens"] == 150
    assert by_model["claude-opus-4-8"]["calls"] == 2
    assert by_model["summarizer"]["prompt_tokens"] == 200  # the compaction call


async def test_usage_daily_buckets_today(client: httpx.AsyncClient, db: Sessions) -> None:
    await _seed(db)
    daily = (await client.get("/api/usage")).json()["daily"]

    assert len(daily) == 1  # everything seeded now -> one day
    assert daily[0]["prompt_tokens"] == 350
    assert daily[0]["calls"] == 2


async def test_run_usage_aggregates_the_whole_team_tree(
    client: httpx.AsyncClient, db: Sessions
) -> None:
    async with session_scope(db) as session:
        root = await queue.enqueue(
            session,
            workspace_id=DEFAULT_WORKSPACE_ID,
            kind=TaskKind.PLAN,
            payload={"goal": "orchestrate", "can_spawn": True},
        )
        # The orchestrator itself barely spends; its worker does the real work.
        await append_event(
            session, root.id, EventType.LLM_RESPONSE, {"model": "m", "usage": _usage(30, 5)}
        )
        child = await queue.enqueue(
            session,
            workspace_id=DEFAULT_WORKSPACE_ID,
            kind=TaskKind.EXECUTE,
            payload={"goal": "do work", "agent_name": "coder"},
            parent=root,
        )
        await append_event(
            session, child.id, EventType.LLM_RESPONSE, {"model": "m", "usage": _usage(400, 90)}
        )

    runs = {r["run_id"]: r for r in (await client.get("/api/usage/runs")).json()["runs"]}
    run = runs[str(root.id)]
    # The team total is root + child, not just the thin orchestrator.
    assert run["prompt_tokens"] == 430
    assert run["completion_tokens"] == 95
    assert run["agents"] == 2  # orchestrator + worker both did LLM work
    assert run["calls"] == 2


async def test_usage_scoped_by_project(client: httpx.AsyncClient, db: Sessions) -> None:
    await _seed(db)
    # A project with no runs sees zero, proving the project filter isolates.
    empty = (await client.get(f"/api/usage?project_id={uuid.uuid4()}")).json()
    assert empty["prompt_tokens"] == 0 and empty["by_model"] == [] and empty["daily"] == []
