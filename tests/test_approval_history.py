"""The No-HITL audit log: /approvals/history surfaces resolved approvals — every
call auto-accepted in No-HITL mode (resolved_by == 'auto-accept') — enriched with
the tool and arguments from the matching request event."""

from __future__ import annotations

import uuid

import httpx
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core.db import session_scope
from gantry.core.events import append_event
from gantry.core.models import EventType

from .test_api import create_task

Sessions = async_sessionmaker[AsyncSession]


async def test_history_lists_an_auto_accepted_decision(
    client: httpx.AsyncClient, db: Sessions
) -> None:
    task = await create_task(client)
    task_id = uuid.UUID(task["id"])
    async with session_scope(db) as session:
        await append_event(
            session,
            task_id,
            EventType.APPROVAL_REQUESTED,
            {"tool_call_id": "c1", "tool": "bash", "arguments": {"command": "ls"}},
        )
        await append_event(
            session,
            task_id,
            EventType.APPROVAL_RESOLVED,
            {"tool_call_id": "c1", "decision": "approved", "resolved_by": "auto-accept"},
        )

    resp = await client.get("/api/approvals/history")
    assert resp.status_code == 200
    items = resp.json()["items"]
    entry = next(i for i in items if i["task_id"] == task["id"])
    # Enriched from the request event, and flagged as a No-HITL auto-accept.
    assert entry["tool"] == "bash"
    assert entry["arguments"] == {"command": "ls"}
    assert entry["decision"] == "approved"
    assert entry["resolved_by"] == "auto-accept"


async def test_history_is_empty_without_resolutions(
    client: httpx.AsyncClient, db: Sessions
) -> None:
    # db triggers the per-test table truncate, so the log starts clean.
    resp = await client.get("/api/approvals/history")
    assert resp.status_code == 200
    assert resp.json()["items"] == []
