"""Agents + teams API: CRUD, whole-tree replace, guards."""

from __future__ import annotations

from typing import Any

import httpx


async def create_agent(client: httpx.AsyncClient, name: str, **overrides: Any) -> dict[str, Any]:
    body: dict[str, Any] = {"name": name, "role": f"{name} role"}
    body.update(overrides)
    response = await client.post("/api/agents", json=body)
    assert response.status_code == 201, response.text
    return response.json()  # type: ignore[no-any-return]


async def create_team(
    client: httpx.AsyncClient, name: str, root: dict[str, Any], description: str = ""
) -> dict[str, Any]:
    response = await client.post(
        "/api/teams", json={"name": name, "description": description, "root": root}
    )
    assert response.status_code == 201, response.text
    return response.json()  # type: ignore[no-any-return]


async def test_agent_crud(client: httpx.AsyncClient) -> None:
    agent = await create_agent(
        client,
        "coder",
        system_prompt="You write code.",
        can_spawn=False,
        gated_tools=["bash"],
        skills=["test-first"],
        max_steps=25,
    )
    assert agent["gated_tools"] == ["bash"]

    listed = (await client.get("/api/agents")).json()["agents"]
    assert [a["name"] for a in listed] == ["coder"]

    updated = await client.put(
        f"/api/agents/{agent['id']}", json={"name": "coder", "role": "better role"}
    )
    assert updated.status_code == 200
    assert updated.json()["role"] == "better role"

    assert (await client.delete(f"/api/agents/{agent['id']}")).status_code == 204
    assert (await client.get("/api/agents")).json()["agents"] == []


async def test_agent_duplicate_name_409(client: httpx.AsyncClient) -> None:
    await create_agent(client, "coder")
    response = await client.post("/api/agents", json={"name": "coder"})
    assert response.status_code == 409


async def test_agent_unknown_provider_422(client: httpx.AsyncClient) -> None:
    response = await client.post(
        "/api/agents",
        json={"name": "x", "provider_id": "00000000-0000-0000-0000-00000000dead"},
    )
    assert response.status_code == 422


async def test_team_create_get_and_replace(client: httpx.AsyncClient) -> None:
    architect = await create_agent(client, "architect", can_spawn=True)
    coder = await create_agent(client, "coder")
    tester = await create_agent(client, "tester")

    team = await create_team(
        client,
        "web crew",
        root={
            "profile_id": architect["id"],
            "children": [
                {"profile_id": coder["id"], "children": []},
                {"profile_id": tester["id"], "children": []},
            ],
        },
    )
    fetched = (await client.get(f"/api/teams/{team['id']}")).json()
    assert fetched["root"]["profile"]["name"] == "architect"
    assert [c["profile"]["name"] for c in fetched["root"]["children"]] == ["coder", "tester"]

    summary = (await client.get("/api/teams")).json()["teams"]
    assert summary[0]["member_count"] == 3

    # Whole-tree replace: drop the tester.
    replaced = await client.put(
        f"/api/teams/{team['id']}",
        json={
            "name": "web crew",
            "description": "smaller",
            "root": {
                "profile_id": architect["id"],
                "children": [{"profile_id": coder["id"], "children": []}],
            },
        },
    )
    assert replaced.status_code == 200
    assert [c["profile"]["name"] for c in replaced.json()["root"]["children"]] == ["coder"]
    assert (await client.get("/api/teams")).json()["teams"][0]["member_count"] == 2


async def test_children_require_can_spawn(client: httpx.AsyncClient) -> None:
    solo = await create_agent(client, "solo")  # can_spawn defaults to False
    child = await create_agent(client, "child")
    response = await client.post(
        "/api/teams",
        json={
            "name": "broken",
            "root": {
                "profile_id": solo["id"],
                "children": [{"profile_id": child["id"], "children": []}],
            },
        },
    )
    assert response.status_code == 422
    assert "can_spawn" in response.json()["detail"]


async def test_delete_agent_in_team_409(client: httpx.AsyncClient) -> None:
    solo = await create_agent(client, "solo")
    team = await create_team(client, "one-person", root={"profile_id": solo["id"], "children": []})
    assert (await client.delete(f"/api/agents/{solo['id']}")).status_code == 409
    # A team owns its agents, so deleting the team deletes them too.
    assert (await client.delete(f"/api/teams/{team['id']}")).status_code == 204
    assert (await client.delete(f"/api/agents/{solo['id']}")).status_code == 404


async def test_teams_have_separate_libraries(client: httpx.AsyncClient) -> None:
    # Each team owns its own agents, so two teams may each have a "coder".
    a1 = await create_agent(client, "coder")
    team_a = await create_team(client, "team-a", root={"profile_id": a1["id"], "children": []})
    a2 = await create_agent(client, "coder")  # allowed: a1 is now owned by team-a
    team_b = await create_team(client, "team-b", root={"profile_id": a2["id"], "children": []})
    ra = await client.get(f"/api/agents?team_id={team_a['id']}")
    rb = await client.get(f"/api/agents?team_id={team_b['id']}")
    assert [a["id"] for a in ra.json()["agents"]] == [a1["id"]]
    assert [a["id"] for a in rb.json()["agents"]] == [a2["id"]]


async def test_team_unknown_profile_422(client: httpx.AsyncClient) -> None:
    response = await client.post(
        "/api/teams",
        json={
            "name": "ghost",
            "root": {"profile_id": "00000000-0000-0000-0000-00000000dead", "children": []},
        },
    )
    assert response.status_code == 422
