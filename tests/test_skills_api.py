"""Skills API: CRUD, project scoping, and DB-backed injection into a run."""

from __future__ import annotations

from typing import Any

import httpx
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.events import read_events
from gantry.core.models import DEFAULT_PROJECT_ID, DEFAULT_WORKSPACE_ID, EventType, Task, TaskKind
from gantry.runtime.loop import run_agent_task
from gantry.runtime.tools import ToolRegistry
from gantry.skills.store import load_registry

from .fakes import ScriptedLLM, final_response

Sessions = async_sessionmaker[AsyncSession]


async def _project(client: httpx.AsyncClient, name: str) -> str:
    return str((await client.post("/api/projects", json={"name": name})).json()["id"])


async def test_skill_crud(client: httpx.AsyncClient) -> None:
    created = (
        await client.post("/api/skills", json={"name": "s1", "body": "B", "match": ["x"]})
    ).json()
    updated = await client.put(
        f"/api/skills/{created['id']}",
        json={"name": "s1", "body": "B2", "match": ["x", "y"]},
    )
    assert updated.status_code == 200
    assert updated.json()["body"] == "B2" and updated.json()["match"] == ["x", "y"]

    assert (await client.delete(f"/api/skills/{created['id']}")).status_code == 204
    assert (await client.get("/api/skills")).json()["skills"] == []


async def test_skills_are_scoped_by_project(client: httpx.AsyncClient) -> None:
    a = await _project(client, "A")
    b = await _project(client, "B")
    # Same skill name allowed in two projects.
    for pid in (a, b):
        resp = await client.post(
            "/api/skills", json={"name": "dup", "project_id": pid, "body": "x"}
        )
        assert resp.status_code == 201, resp.text

    in_a = (await client.get(f"/api/skills?project_id={a}")).json()["skills"]
    assert [s["name"] for s in in_a] == ["dup"]
    assert in_a[0]["project_id"] == a


async def test_duplicate_name_in_same_project_409(client: httpx.AsyncClient) -> None:
    await client.post("/api/skills", json={"name": "dup", "body": "x"})
    resp = await client.post("/api/skills", json={"name": "dup", "body": "y"})
    assert resp.status_code == 409


async def test_db_skill_injects_into_a_run(client: httpx.AsyncClient, db: Sessions) -> None:
    """A skill authored via the API is loaded from the DB and injected."""
    await client.post(
        "/api/skills",
        json={"name": "widgets", "body": "WIDGET RULES", "match": ["widget"]},
    )
    async with session_scope(db) as session:
        task: Task = await queue.enqueue(
            session,
            workspace_id=DEFAULT_WORKSPACE_ID,
            project_id=DEFAULT_PROJECT_ID,
            kind=TaskKind.EXECUTE,
            payload={"model": "fake/test", "goal": "improve the widget"},
        )
    async with db() as session:
        registry = await load_registry(
            session, workspace_id=DEFAULT_WORKSPACE_ID, project_id=DEFAULT_PROJECT_ID
        )
    llm = ScriptedLLM([final_response("done")])
    await run_agent_task(db, task, llm, ToolRegistry([]), skills=registry)

    system = llm.calls[0]["messages"][0]
    assert "WIDGET RULES" in system["content"]
    async with session_scope(db) as session:
        events = await read_events(session, task.id)
    injected: list[Any] = [e for e in events if e.event_type is EventType.SKILL_INJECTED]
    assert len(injected) == 1 and injected[0].payload["content"] == "WIDGET RULES"
