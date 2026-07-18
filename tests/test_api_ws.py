"""WebSocket streaming tests: replay, live tail, reconnect, and the flagship —
watching a real worker deliver a coding task through the socket, live."""

from __future__ import annotations

import asyncio
import json
import uuid
from contextlib import AbstractAsyncContextManager
from pathlib import Path
from typing import Any, cast

import httpx
import pytest
from httpx_ws import AsyncWebSocketSession, WebSocketDisconnect, aconnect_ws
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.events import append_event
from gantry.core.models import EventType
from gantry.worker.service import Worker, WorkerConfig

from .fakes import ScriptedLLM, final_response, response_with_tool_call
from .test_api import create_task
from .test_worker_git import git, origin  # noqa: F401  (fixture re-export)

Sessions = async_sessionmaker[AsyncSession]

BASE = "http://gantry.test"


def connect(
    client: httpx.AsyncClient, path: str
) -> AbstractAsyncContextManager[AsyncWebSocketSession]:
    return cast(
        "AbstractAsyncContextManager[AsyncWebSocketSession]",
        aconnect_ws(f"{BASE}{path}", client),
    )


async def recv(ws: AsyncWebSocketSession, timeout_seconds: float = 10) -> dict[str, Any]:
    return cast("dict[str, Any]", json.loads(await ws.receive_text(timeout=timeout_seconds)))


async def recv_event(ws: AsyncWebSocketSession, timeout_seconds: float = 10) -> dict[str, Any]:
    """Next event message, skipping interleaved task snapshots."""
    while True:
        message = await recv(ws, timeout_seconds)
        if message["type"] == "event":
            return message


async def test_ws_sends_snapshot_then_replays_events(
    client: httpx.AsyncClient, db: Sessions
) -> None:
    task = await create_task(client)
    task_id = uuid.UUID(task["id"])
    async with session_scope(db) as session:
        await append_event(session, task_id, EventType.LLM_REQUEST, {"n": 1})

    async with connect(client, f"/api/tasks/{task_id}/events/ws") as ws:
        snapshot = await recv(ws)
        assert snapshot["type"] == "task"
        assert snapshot["data"]["id"] == task["id"]
        assert snapshot["data"]["status"] == "pending"

        first, second = await recv(ws), await recv(ws)
        assert (first["data"]["seq"], first["data"]["event_type"]) == (1, "task_enqueued")
        assert (second["data"]["seq"], second["data"]["event_type"]) == (2, "llm_request")


async def test_ws_resumes_from_client_cursor(client: httpx.AsyncClient, db: Sessions) -> None:
    task = await create_task(client)
    task_id = uuid.UUID(task["id"])
    async with session_scope(db) as session:
        for n in (1, 2, 3):
            await append_event(session, task_id, EventType.TERMINAL_CHUNK, {"n": n})

    async with connect(client, f"/api/tasks/{task_id}/events/ws?after_seq=2") as ws:
        assert (await recv(ws))["type"] == "task"
        seqs = [(await recv(ws))["data"]["seq"] for _ in range(2)]
        assert seqs == [3, 4]  # nothing at or before the cursor is re-sent


async def test_ws_delivers_new_events_via_notify_not_poll(
    client: httpx.AsyncClient, db: Sessions
) -> None:
    task = await create_task(client)
    task_id = uuid.UUID(task["id"])
    async with connect(client, f"/api/tasks/{task_id}/events/ws") as ws:
        await recv(ws)  # snapshot
        await recv(ws)  # task_enqueued

        started = asyncio.get_running_loop().time()
        async with session_scope(db) as session:
            await append_event(session, task_id, EventType.TERMINAL_CHUNK, {"data": "live!"})
        message = await recv_event(ws)
        elapsed = asyncio.get_running_loop().time() - started

        assert message["data"]["event_type"] == "terminal_chunk"
        assert message["data"]["payload"] == {"data": "live!"}
        # The poll fallback is jittered around 5s; NOTIFY delivery is ~ms.
        assert elapsed < 3.5, f"event took {elapsed:.1f}s — poll fallback, not NOTIFY?"


async def test_ws_closes_cleanly_when_task_reaches_terminal_status(
    client: httpx.AsyncClient, db: Sessions
) -> None:
    task = await create_task(client)
    async with connect(client, f"/api/tasks/{task['id']}/events/ws") as ws:
        await recv(ws)  # snapshot
        async with session_scope(db) as session:
            claimed = await queue.claim(session, worker_id="w1", lease_seconds=30)
        assert claimed is not None
        async with session_scope(db) as session:
            assert await queue.complete(
                session, task_id=claimed.id, worker_id="w1", attempt=claimed.attempt
            )

        event_types, last_snapshot = [], None
        with pytest.raises(WebSocketDisconnect) as disconnect:
            while True:
                message = await recv(ws)
                if message["type"] == "event":
                    event_types.append(message["data"]["event_type"])
                else:
                    last_snapshot = message["data"]
        assert disconnect.value.code == 1000
    assert last_snapshot is not None and last_snapshot["status"] == "succeeded"
    assert event_types[-1] == "task_succeeded"


async def test_ws_unknown_task_closes_with_4404(client: httpx.AsyncClient) -> None:
    async with connect(client, f"/api/tasks/{uuid.uuid4()}/events/ws") as ws:
        with pytest.raises(WebSocketDisconnect) as disconnect:
            await ws.receive_text(timeout=5)
        assert disconnect.value.code == 4404


async def test_firehose_streams_events_across_tasks(client: httpx.AsyncClient) -> None:
    async with connect(client, "/api/events/ws") as ws:
        one = await create_task(client)
        two = await create_task(client)
        seen: set[str] = set()
        while seen != {one["id"], two["id"]}:
            message = await recv(ws)
            if message["data"]["event_type"] == "task_enqueued":
                seen.add(message["data"]["task_id"])


async def test_firehose_replays_from_global_cursor(client: httpx.AsyncClient) -> None:
    task = await create_task(client)  # enqueued before the socket ever connects
    async with connect(client, "/api/events/ws?after_id=0") as ws:
        message = await recv(ws)
        assert message["data"]["task_id"] == task["id"]
        assert message["data"]["event_type"] == "task_enqueued"


async def test_watch_a_live_agent_run_through_the_websocket(
    client: httpx.AsyncClient,
    db: Sessions,
    origin: Path,  # noqa: F811
    tmp_path: Path,
) -> None:
    """Phase 4 acceptance: every durable step of a real worker's coding task —
    LLM turns, tool calls, terminal output, git delivery — arrives live over
    one websocket, ending with a succeeded snapshot and a clean close."""
    task = await create_task(client, goal="deliver a greeting", repo_url=str(origin))
    llm = ScriptedLLM(
        [
            response_with_tool_call(
                "c1", "write_file", {"path": "hello.txt", "content": "hello from gantry\n"}
            ),
            response_with_tool_call("c2", "bash", {"command": "cat hello.txt"}),
            response_with_tool_call("c3", "git_commit_push", {"message": "add greeting"}),
            final_response("delivered"),
        ]
    )
    config = WorkerConfig(
        worker_id="ws-e2e", workspace_root=tmp_path / "workspaces", lease_seconds=30
    )
    worker = Worker(db, config, llm)

    async with connect(client, f"/api/tasks/{task['id']}/events/ws") as ws:
        assert (await recv(ws))["type"] == "task"

        async def drive() -> None:
            async with session_scope(db) as session:
                claimed = await queue.claim(session, worker_id="ws-e2e", lease_seconds=30)
            assert claimed is not None
            await worker.process(claimed)

        driver = asyncio.create_task(drive())
        try:
            event_types, seqs, last_snapshot = [], [], None
            with pytest.raises(WebSocketDisconnect) as disconnect:
                while True:
                    message = await recv(ws, timeout_seconds=30)
                    if message["type"] == "event":
                        event_types.append(message["data"]["event_type"])
                        seqs.append(message["data"]["seq"])
                    else:
                        last_snapshot = message["data"]
            assert disconnect.value.code == 1000
        finally:
            await driver

    assert last_snapshot is not None
    assert last_snapshot["status"] == "succeeded"
    assert last_snapshot["result"]["final_text"] == "delivered"
    for expected in (
        "task_enqueued",
        "task_claimed",
        "llm_request",
        "llm_response",
        "tool_call",
        "terminal_chunk",
        "tool_result",
        "task_succeeded",
    ):
        assert expected in event_types, f"never saw {expected} (got {event_types})"
    assert seqs == sorted(seqs) and len(seqs) == len(set(seqs))  # ordered, no duplicates

    # And the work really landed on the remote, as always.
    branch = last_snapshot["result"]["branch"]
    assert git("--git-dir", str(origin), "show", f"{branch}:hello.txt") == "hello from gantry"
