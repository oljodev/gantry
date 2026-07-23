"""Co-pilot: restricted toolset, proposal events, and session start."""

from __future__ import annotations

import uuid
from typing import Any

import httpx
import sqlalchemy as sa
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.models import DEFAULT_WORKSPACE_ID, EventType, Skill, TaskKind
from gantry.runtime.tools import ToolContext
from gantry.worker.tools import build_copilot_registry
from gantry.worker.tools.copilot import CreateSkillTool, ProposeSkillTool, ProposeTreeTool

Sessions = async_sessionmaker[AsyncSession]


class Emitted:
    def __init__(self) -> None:
        self.events: list[tuple[EventType, dict[str, Any]]] = []

    async def __call__(self, event_type: EventType, payload: dict[str, Any]) -> int:
        self.events.append((event_type, payload))
        return len(self.events)


def _names(kind: str) -> set[str]:
    return {s["function"]["name"] for s in build_copilot_registry(kind).schemas()}


def test_tree_architect_allocates_models_by_cost_tier() -> None:
    from gantry.worker.tools.copilot import TREE_ARCHITECT_PROMPT

    prompt = TREE_ARCHITECT_PROMPT.lower()
    # Cheap models for read-only work, reasoning models for hard work, strong
    # models for orchestration — the cost-tiering the co-pilot should apply.
    assert "cheapest" in prompt and "read-only" in prompt
    assert "reasoning model" in prompt
    assert "orchestrator" in prompt


def test_tree_architect_designs_swarm_leaders_with_qa() -> None:
    from gantry.worker.tools.copilot import TREE_ARCHITECT_PROMPT

    prompt = TREE_ARCHITECT_PROMPT.lower()
    # Leaders delegate in tiny parallel micro-tasks...
    assert "micro-task" in prompt
    # ...and every code-writing team gets a sequential qa-reviewer node.
    assert "qa-reviewer" in prompt


def test_copilot_registry_is_restricted() -> None:
    assert _names("skill") == {"propose_skill", "ask_user"}
    # The tree co-pilot can also author skills for the team (gated by approval).
    assert _names("tree") == {"propose_tree", "create_skill", "ask_user"}
    # None of the powerful tools leak into a co-pilot.
    assert "bash" not in _names("skill") and "spawn_subtask" not in _names("tree")


async def test_propose_skill_emits_a_proposal() -> None:
    emitted = Emitted()
    ctx = ToolContext(task_id=uuid.uuid4(), emit_event=emitted, tool_call_id="c1")
    result = await ProposeSkillTool().execute(
        {"name": "widgets", "description": "d", "match": ["widget"], "body": "RULES"}, ctx
    )
    assert not result.is_error
    kind, payload = emitted.events[0]
    assert kind is EventType.COPILOT_PROPOSAL
    assert payload["kind"] == "skill"
    assert payload["skill"]["name"] == "widgets" and payload["skill"]["body"] == "RULES"


async def test_propose_skill_requires_name_and_body() -> None:
    emitted = Emitted()
    ctx = ToolContext(task_id=uuid.uuid4(), emit_event=emitted, tool_call_id="c1")
    result = await ProposeSkillTool().execute({"name": "x"}, ctx)  # no body
    assert result.is_error and not emitted.events


async def test_propose_tree_emits_a_proposal() -> None:
    emitted = Emitted()
    ctx = ToolContext(task_id=uuid.uuid4(), emit_event=emitted, tool_call_id="c1")
    root = {"name": "architect", "role": "plans", "can_spawn": True, "children": []}
    result = await ProposeTreeTool().execute({"name": "My Team", "root": root}, ctx)
    assert not result.is_error
    kind, payload = emitted.events[0]
    assert kind is EventType.COPILOT_PROPOSAL and payload["kind"] == "tree"
    assert payload["team"]["name"] == "My Team"
    assert payload["team"]["root"]["name"] == "architect"


async def test_start_copilot_creates_a_restricted_task(client: httpx.AsyncClient) -> None:
    resp = await client.post(
        "/api/copilot",
        json={"kind": "skill", "instruction": "a skill for writing good commits"},
    )
    assert resp.status_code == 201, resp.text
    task = resp.json()
    assert task["payload"]["copilot"] == "skill"
    assert "commit" in task["payload"]["goal"]
    assert task["payload"]["system_prompt"]  # architect prompt attached


async def test_copilot_tasks_are_hidden_from_the_runs_list(client: httpx.AsyncClient) -> None:
    started = await client.post(
        "/api/copilot", json={"kind": "skill", "instruction": "a commit skill"}
    )
    assert started.status_code == 201
    copilot_id = started.json()["id"]

    listed = await client.get("/api/tasks")
    assert listed.status_code == 200
    assert copilot_id not in [t["id"] for t in listed.json()["tasks"]]


async def test_start_copilot_includes_editor_context(client: httpx.AsyncClient) -> None:
    resp = await client.post(
        "/api/copilot",
        json={"kind": "tree", "instruction": "build a team", "context": "EXISTING TREE JSON"},
    )
    assert resp.status_code == 201
    assert "EXISTING TREE JSON" in resp.json()["payload"]["goal"]


async def test_saved_session_records_turns(client: httpx.AsyncClient) -> None:
    created = await client.post("/api/copilot/sessions", json={"kind": "tree"})
    assert created.status_code == 201, created.text
    session_id = created.json()["id"]
    assert created.json()["turns"] == []

    started = await client.post(
        "/api/copilot",
        json={"kind": "tree", "instruction": "design a review crew", "session_id": session_id},
    )
    assert started.status_code == 201
    task_id = started.json()["id"]

    got = await client.get(f"/api/copilot/sessions/{session_id}")
    body = got.json()
    assert [t["task_id"] for t in body["turns"]] == [task_id]
    assert body["turns"][0]["user"] == "design a review crew"
    assert body["title"]  # auto-titled from the first message

    listed = await client.get("/api/copilot/sessions?kind=tree")
    assert session_id in [s["id"] for s in listed.json()["sessions"]]

    assert (await client.delete(f"/api/copilot/sessions/{session_id}")).status_code == 204
    assert (await client.get(f"/api/copilot/sessions/{session_id}")).status_code == 404


async def test_create_skill_tool_saves_to_the_project(db: Sessions) -> None:
    async with session_scope(db) as session:
        task = await queue.enqueue(
            session,
            workspace_id=DEFAULT_WORKSPACE_ID,
            kind=TaskKind.EXECUTE,
            payload={"goal": "build a team", "copilot": "tree"},
        )
    ctx = ToolContext(task_id=task.id, sessions=db, tool_call_id="c1")
    result = await CreateSkillTool().execute(
        {"name": "pr-hygiene", "description": "d", "match": ["pr"], "body": "RULES"}, ctx
    )
    assert not result.is_error, result.content
    async with session_scope(db) as session:
        rows = (await session.scalars(sa.select(Skill).where(Skill.name == "pr-hygiene"))).all()
    assert len(rows) == 1
    assert rows[0].body == "RULES" and rows[0].project_id == task.project_id

    # Re-running (crash recovery after approval) upserts by name, never duplicates.
    ctx2 = ToolContext(task_id=task.id, sessions=db, tool_call_id="c1")
    await CreateSkillTool().execute({"name": "pr-hygiene", "body": "NEW"}, ctx2)
    async with session_scope(db) as session:
        rows = (await session.scalars(sa.select(Skill).where(Skill.name == "pr-hygiene"))).all()
    assert len(rows) == 1 and rows[0].body == "NEW"


async def test_tree_copilot_gates_create_skill(client: httpx.AsyncClient) -> None:
    resp = await client.post("/api/copilot", json={"kind": "tree", "instruction": "build a crew"})
    assert resp.status_code == 201
    assert resp.json()["payload"]["gated_tools"] == ["create_skill"]
