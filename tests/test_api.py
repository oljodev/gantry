"""Control plane REST tests — the full app (lifespan, broker, reaper) over ASGI."""

from __future__ import annotations

import asyncio
import uuid
from typing import Any, cast

import httpx
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.events import append_event
from gantry.core.models import EventType

Sessions = async_sessionmaker[AsyncSession]


async def create_task(client: httpx.AsyncClient, **overrides: object) -> dict[str, Any]:
    body = {"goal": "write a haiku about cranes", **overrides}
    response = await client.post("/api/tasks", json=body)
    assert response.status_code == 201, response.text
    return cast("dict[str, Any]", response.json())


async def test_create_task_builds_payload_and_defaults(client: httpx.AsyncClient) -> None:
    task = await create_task(
        client,
        repo_url="git@example.com:demo/repo.git",
        model="anthropic/claude-opus-4-8",
        priority=5,
        payload={"custom": "field"},
    )
    assert task["status"] == "pending"
    assert task["kind"] == "execute"
    assert task["priority"] == 5
    assert task["payload"] == {
        "custom": "field",
        "goal": "write a haiku about cranes",
        "repo_url": "git@example.com:demo/repo.git",
        "model": "anthropic/claude-opus-4-8",
    }
    assert task["root_task_id"] == task["id"]  # root task is its own tree root


async def test_create_task_requires_a_goal(client: httpx.AsyncClient) -> None:
    response = await client.post("/api/tasks", json={"goal": ""})
    assert response.status_code == 422


async def test_get_and_list_tasks(client: httpx.AsyncClient) -> None:
    created = await create_task(client)

    got = await client.get(f"/api/tasks/{created['id']}")
    assert got.status_code == 200
    assert got.json() == created

    listed = await client.get("/api/tasks")
    assert [t["id"] for t in listed.json()["tasks"]] == [created["id"]]

    # Status filter matches ... and excludes.
    assert len((await client.get("/api/tasks", params={"status": "pending"})).json()["tasks"]) == 1
    assert (await client.get("/api/tasks", params={"status": "failed"})).json()["tasks"] == []


async def test_get_unknown_task_is_404(client: httpx.AsyncClient) -> None:
    assert (await client.get(f"/api/tasks/{uuid.uuid4()}")).status_code == 404
    assert (await client.get(f"/api/tasks/{uuid.uuid4()}/events")).status_code == 404
    assert (await client.post(f"/api/tasks/{uuid.uuid4()}/cancel")).status_code == 404


async def test_task_events_endpoint_pages_by_seq(client: httpx.AsyncClient, db: Sessions) -> None:
    task = await create_task(client)
    task_id = uuid.UUID(task["id"])
    async with session_scope(db) as session:
        for i in range(3):
            await append_event(session, task_id, EventType.LLM_REQUEST, {"i": i})

    events = (await client.get(f"/api/tasks/{task_id}/events")).json()["events"]
    assert [e["seq"] for e in events] == [1, 2, 3, 4]
    assert events[0]["event_type"] == "task_enqueued"

    after = (
        await client.get(f"/api/tasks/{task_id}/events", params={"after_seq": 2, "limit": 1})
    ).json()["events"]
    assert [e["seq"] for e in after] == [3]


async def test_cancel_pending_is_immediate(client: httpx.AsyncClient, db: Sessions) -> None:
    task = await create_task(client)

    cancelled = await client.post(f"/api/tasks/{task['id']}/cancel")
    assert cancelled.status_code == 200
    assert cancelled.json()["status"] == "cancelled"
    events = (await client.get(f"/api/tasks/{task['id']}/events")).json()["events"]
    assert events[-1]["event_type"] == "task_cancelled"

    # Cancelling a terminal task conflicts.
    assert (await client.post(f"/api/tasks/{task['id']}/cancel")).status_code == 409


async def test_cancel_running_requests_cooperative_stop(
    client: httpx.AsyncClient, db: Sessions
) -> None:
    await create_task(client)
    async with session_scope(db) as session:
        claimed = await queue.claim(session, worker_id="w1", lease_seconds=30)
    assert claimed is not None

    # A live (leased) task can't be yanked terminal from under its worker; the
    # endpoint flags it for cooperative stop and leaves the status alone.
    resp = await client.post(f"/api/tasks/{claimed.id}/cancel")
    assert resp.status_code == 200
    body = resp.json()
    assert body["status"] == "claimed"
    assert body["cancel_requested"] is True

    # The worker honours the flag: its heartbeat now reports cancel, and it
    # transitions the task to CANCELLED via mark_cancelled.
    async with session_scope(db) as session:
        beat = await queue.heartbeat(session, task_id=claimed.id, worker_id="w1", attempt=1)
        assert beat.alive and beat.cancel_requested
        assert await queue.mark_cancelled(session, task_id=claimed.id, worker_id="w1", attempt=1)
    refreshed = (await client.get(f"/api/tasks/{claimed.id}")).json()
    assert refreshed["status"] == "cancelled"


async def test_stats_reflect_fleet_state(client: httpx.AsyncClient, db: Sessions) -> None:
    await create_task(client)
    second = await create_task(client)
    async with session_scope(db) as session:
        claimed = await queue.claim(session, worker_id="stats-w", lease_seconds=30)
    assert claimed is not None
    async with session_scope(db) as session:
        assert await queue.complete(
            session,
            task_id=claimed.id,
            worker_id="stats-w",
            attempt=claimed.attempt,
            result={"final_text": "ok", "prompt_tokens": 120, "completion_tokens": 45},
        )

    stats = (await client.get("/api/stats")).json()
    assert stats["total"] == 2
    assert stats["statuses"] == {"pending": 1, "succeeded": 1}
    assert stats["recent_workers"] == 1  # stats-w claimed within the window
    assert stats["prompt_tokens"] == 120 and stats["completion_tokens"] == 45
    assert stats["events_last_hour"] >= 4
    del second


async def test_retry_requeues_failed_task_with_headroom(
    client: httpx.AsyncClient, db: Sessions
) -> None:
    task = await create_task(client)
    async with session_scope(db) as session:
        claimed = await queue.claim(session, worker_id="w1", lease_seconds=30)
    assert claimed is not None
    async with session_scope(db) as session:
        status = await queue.fail(
            session,
            task_id=claimed.id,
            worker_id="w1",
            attempt=claimed.attempt,
            error="boom",
            retryable=False,
        )
    assert status is not None and status.value == "failed"

    retried = await client.post(f"/api/tasks/{task['id']}/retry")
    assert retried.status_code == 200
    body = retried.json()
    assert body["status"] == "pending"
    assert body["max_attempts"] >= body["attempt"] + 3  # real headroom for the retry

    # Only failed/cancelled tasks are retryable.
    assert (await client.post(f"/api/tasks/{task['id']}/retry")).status_code == 409
    assert (await client.post(f"/api/tasks/{uuid.uuid4()}/retry")).status_code == 404


async def test_server_reaper_requeues_expired_leases(
    client: httpx.AsyncClient, db: Sessions
) -> None:
    """The control plane's background reaper recovers a died-worker task."""
    task = await create_task(client)
    async with session_scope(db) as session:
        claimed = await queue.claim(session, worker_id="doomed", lease_seconds=0.05)
    assert claimed is not None and str(claimed.id) == task["id"]

    deadline = asyncio.get_running_loop().time() + 10
    while True:
        status = (await client.get(f"/api/tasks/{task['id']}")).json()["status"]
        if status == "pending":
            break
        assert asyncio.get_running_loop().time() < deadline, f"never reaped (status={status})"
        await asyncio.sleep(0.1)

    events = (await client.get(f"/api/tasks/{task['id']}/events")).json()["events"]
    assert [e["event_type"] for e in events][-1] == "task_lease_expired"
