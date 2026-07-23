"""Co-pilot: restricted toolset, proposal events, and session start."""

from __future__ import annotations

import uuid
from typing import Any

import httpx

from gantry.core.models import EventType
from gantry.runtime.tools import ToolContext
from gantry.worker.tools import build_copilot_registry
from gantry.worker.tools.copilot import ProposeSkillTool, ProposeTreeTool


class Emitted:
    def __init__(self) -> None:
        self.events: list[tuple[EventType, dict[str, Any]]] = []

    async def __call__(self, event_type: EventType, payload: dict[str, Any]) -> int:
        self.events.append((event_type, payload))
        return len(self.events)


def _names(kind: str) -> set[str]:
    return {s["function"]["name"] for s in build_copilot_registry(kind).schemas()}


def test_copilot_registry_is_restricted() -> None:
    assert _names("skill") == {"propose_skill", "ask_user"}
    assert _names("tree") == {"propose_tree", "ask_user"}
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
