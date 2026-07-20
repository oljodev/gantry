"""Team launch: snapshot immutability, roster prompts, agent-directed spawns."""

from __future__ import annotations

from typing import Any

import httpx
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.models import Task, TaskKind, TaskStatus
from gantry.runtime.tools import ToolContext
from gantry.worker.tools.orchestration import SpawnSubtaskTool, child_task_id

from .test_agents_teams_api import create_agent, create_team
from .test_providers_api import create_provider

Sessions = async_sessionmaker[AsyncSession]


async def launch(client: httpx.AsyncClient, team_id: str, **body: Any) -> dict[str, Any]:
    response = await client.post(
        f"/api/teams/{team_id}/launch", json={"goal": "ship the feature", **body}
    )
    assert response.status_code == 201, response.text
    return response.json()  # type: ignore[no-any-return]


async def build_crew(client: httpx.AsyncClient) -> tuple[dict[str, Any], dict[str, Any]]:
    """architect(plan) -> [coder(gated bash), reviewer] with a provider on coder."""
    provider = await create_provider(
        client, name="router", provider_type="openrouter", default_model="qwen/qwen3-coder"
    )
    architect = await create_agent(
        client, "architect", can_spawn=True, system_prompt="You are the architect."
    )
    coder = await create_agent(
        client,
        "coder",
        role="implements features",
        provider_id=provider["id"],
        gated_tools=["bash"],
        skills=["test-first"],
        max_steps=30,
    )
    reviewer = await create_agent(client, "reviewer", role="reviews diffs")
    team = await create_team(
        client,
        "crew",
        root={
            "profile_id": architect["id"],
            "children": [
                {"profile_id": coder["id"], "children": []},
                {"profile_id": reviewer["id"], "children": []},
            ],
        },
    )
    return team, provider


async def test_launch_builds_snapshot_payload(client: httpx.AsyncClient) -> None:
    team, provider = await build_crew(client)
    task = await launch(client, team["id"], repo_url="git@github.com:o/r.git")

    assert task["kind"] == "plan"
    payload = task["payload"]
    assert payload["goal"] == "ship the feature"
    assert payload["repo_url"] == "git@github.com:o/r.git"
    # Custom root prompt + roster appendix naming the delegates.
    assert payload["system_prompt"].startswith("You are the architect.")
    assert "coder: implements features" in payload["system_prompt"]
    assert "reviewer: reviews diffs" in payload["system_prompt"]
    # The full recursive snapshot rides in the payload.
    team_node = payload["team"]
    assert [c["name"] for c in team_node["children"]] == ["coder", "reviewer"]
    coder_node = team_node["children"][0]
    assert coder_node["model"] == "openrouter/qwen/qwen3-coder"  # litellm-mapped at launch
    assert coder_node["provider_id"] == provider["id"]
    assert coder_node["gated_tools"] == ["bash"]
    # No secret material anywhere near the payload.
    assert "sk-test" not in str(payload)


async def test_snapshot_is_immune_to_profile_edits(client: httpx.AsyncClient) -> None:
    team, _ = await build_crew(client)
    task = await launch(client, team["id"])

    agents = (await client.get("/api/agents")).json()["agents"]
    coder = next(a for a in agents if a["name"] == "coder")
    coder.update(system_prompt="EVIL NEW PROMPT", role="changed")
    assert (await client.put(f"/api/agents/{coder['id']}", json=coder)).status_code == 200

    refetched = (await client.get(f"/api/tasks/{task['id']}")).json()
    assert "EVIL NEW PROMPT" not in str(refetched["payload"])
    assert refetched["payload"] == task["payload"]


async def test_single_agent_team_is_execute(client: httpx.AsyncClient) -> None:
    solo = await create_agent(client, "solo", system_prompt="Just do it.")
    team = await create_team(client, "solo-team", root={"profile_id": solo["id"], "children": []})
    task = await launch(client, team["id"])
    assert task["kind"] == "execute"
    assert task["payload"]["system_prompt"] == "Just do it."
    assert "team" not in task["payload"]


# --- spawn_subtask with `agent` -------------------------------------------


async def claim_planner(db: Sessions, task_id: str) -> Task:
    async with session_scope(db) as session:
        claimed = await queue.claim(session, worker_id="launch-w", lease_seconds=30)
    assert claimed is not None and str(claimed.id) == task_id
    return claimed


async def test_spawn_with_agent_uses_snapshot(client: httpx.AsyncClient, db: Sessions) -> None:
    team, provider = await build_crew(client)
    root = await launch(client, team["id"], repo_url="git@github.com:o/r.git")
    planner = await claim_planner(db, root["id"])

    tool = SpawnSubtaskTool(team=planner.payload["team"])
    ctx = ToolContext(task_id=planner.id, sessions=db, tool_call_id="call-1")
    result = await tool.execute({"goal": "write the code", "agent": "coder"}, ctx)
    assert not result.is_error, result.content

    child_id = child_task_id(planner.id, "call-1")
    async with db() as session:
        child = await session.get(Task, child_id)
    assert child is not None
    assert child.kind is TaskKind.EXECUTE
    assert child.status is TaskStatus.PENDING
    payload = child.payload
    assert payload["goal"] == "write the code"
    assert payload["model"] == "openrouter/qwen/qwen3-coder"
    assert payload["provider_id"] == provider["id"]
    assert payload["gated_tools"] == ["bash"]
    assert payload["skills"] == ["test-first"]
    assert payload["max_steps"] == 30
    # Repo inherited from the parent tree.
    assert payload["repo_url"] == "git@github.com:o/r.git"

    # Idempotency: crash-recovery re-run converges on the same child.
    rerun = await tool.execute({"goal": "write the code", "agent": "coder"}, ctx)
    assert "already spawned" in rerun.content


async def test_spawn_with_unknown_agent_errors(client: httpx.AsyncClient, db: Sessions) -> None:
    team, _ = await build_crew(client)
    root = await launch(client, team["id"])
    planner = await claim_planner(db, root["id"])

    tool = SpawnSubtaskTool(team=planner.payload["team"])
    ctx = ToolContext(task_id=planner.id, sessions=db, tool_call_id="call-x")
    result = await tool.execute({"goal": "x", "agent": "ghostwriter"}, ctx)
    assert result.is_error
    assert "coder" in result.content and "reviewer" in result.content


async def test_spawn_explicit_model_overrides_snapshot(
    client: httpx.AsyncClient, db: Sessions
) -> None:
    team, _ = await build_crew(client)
    root = await launch(client, team["id"])
    planner = await claim_planner(db, root["id"])

    tool = SpawnSubtaskTool(team=planner.payload["team"])
    ctx = ToolContext(task_id=planner.id, sessions=db, tool_call_id="call-2")
    result = await tool.execute(
        {"goal": "x", "agent": "coder", "model": "anthropic/claude-opus-4-8"}, ctx
    )
    assert not result.is_error
    async with db() as session:
        child = await session.get(Task, child_task_id(planner.id, "call-2"))
    assert child is not None
    assert child.payload["model"] == "anthropic/claude-opus-4-8"


async def test_nested_team_child_gets_its_subtree(client: httpx.AsyncClient, db: Sessions) -> None:
    lead = await create_agent(client, "lead", can_spawn=True)
    sub_lead = await create_agent(client, "sub-lead", role="runs the subteam", can_spawn=True)
    worker_agent = await create_agent(client, "grunt", role="does the work")
    team = await create_team(
        client,
        "nested",
        root={
            "profile_id": lead["id"],
            "children": [
                {
                    "profile_id": sub_lead["id"],
                    "children": [{"profile_id": worker_agent["id"], "children": []}],
                }
            ],
        },
    )
    root = await launch(client, team["id"])
    planner = await claim_planner(db, root["id"])

    tool = SpawnSubtaskTool(team=planner.payload["team"])
    ctx = ToolContext(task_id=planner.id, sessions=db, tool_call_id="call-3")
    result = await tool.execute({"goal": "run the subteam", "agent": "sub-lead"}, ctx)
    assert not result.is_error
    async with db() as session:
        child = await session.get(Task, child_task_id(planner.id, "call-3"))
    assert child is not None
    # The sub-lead is itself a planner and carries its own subtree + roster.
    assert child.kind is TaskKind.PLAN
    assert [c["name"] for c in child.payload["team"]["children"]] == ["grunt"]
    assert "grunt: does the work" in child.payload["system_prompt"]
