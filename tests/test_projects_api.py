"""Projects API: CRUD, rollup counts, scoping isolation, delete guards."""

from __future__ import annotations

import httpx

_DEFAULT = "00000000-0000-0000-0000-000000000002"


async def _new_project(client: httpx.AsyncClient, name: str) -> str:
    resp = await client.post("/api/projects", json={"name": name})
    assert resp.status_code == 201, resp.text
    return str(resp.json()["id"])


async def test_project_crud_and_default_exists(client: httpx.AsyncClient) -> None:
    listed = (await client.get("/api/projects")).json()["projects"]
    assert [p["name"] for p in listed] == ["Default"]

    pid = await _new_project(client, "Chess")
    updated = await client.put(
        f"/api/projects/{pid}",
        json={"name": "Chess", "description": "a chess engine", "default_repo_url": "x"},
    )
    assert updated.status_code == 200
    assert updated.json()["description"] == "a chess engine"

    names = {p["name"] for p in (await client.get("/api/projects")).json()["projects"]}
    assert names == {"Default", "Chess"}


async def test_duplicate_name_409(client: httpx.AsyncClient) -> None:
    await _new_project(client, "Dup")
    resp = await client.post("/api/projects", json={"name": "Dup"})
    assert resp.status_code == 409


async def test_runs_are_scoped_by_project(client: httpx.AsyncClient) -> None:
    a = await _new_project(client, "A")
    b = await _new_project(client, "B")
    ra = await client.post("/api/tasks", json={"goal": "in A", "project_id": a})
    assert ra.status_code == 201
    await client.post("/api/tasks", json={"goal": "in B", "project_id": b})

    in_a = (await client.get(f"/api/tasks?project_id={a}")).json()["tasks"]
    assert [t["payload"]["goal"] for t in in_a] == ["in A"]
    assert all(t["project_id"] == a for t in in_a)

    in_b = (await client.get(f"/api/tasks?project_id={b}")).json()["tasks"]
    assert [t["payload"]["goal"] for t in in_b] == ["in B"]

    # Rollup counts reflect the scoping.
    projects = {p["name"]: p for p in (await client.get("/api/projects")).json()["projects"]}
    assert projects["A"]["run_count"] == 1
    assert projects["B"]["run_count"] == 1


async def test_agents_are_scoped_by_project(client: httpx.AsyncClient) -> None:
    a = await _new_project(client, "A")
    b = await _new_project(client, "B")
    # Same agent name in two projects is allowed (unique per project).
    for pid in (a, b):
        resp = await client.post("/api/agents", json={"name": "coder", "project_id": pid})
        assert resp.status_code == 201, resp.text

    in_a = (await client.get(f"/api/agents?project_id={a}")).json()["agents"]
    assert [x["name"] for x in in_a] == ["coder"]
    assert in_a[0]["project_id"] == a


async def test_default_project_cannot_be_deleted(client: httpx.AsyncClient) -> None:
    resp = await client.delete(f"/api/projects/{_DEFAULT}")
    assert resp.status_code == 409


async def test_project_with_runs_wont_delete(client: httpx.AsyncClient) -> None:
    pid = await _new_project(client, "Busy")
    await client.post("/api/tasks", json={"goal": "x", "project_id": pid})
    resp = await client.delete(f"/api/projects/{pid}")
    assert resp.status_code == 409 and "runs" in resp.json()["detail"]


async def test_empty_project_deletes(client: httpx.AsyncClient) -> None:
    pid = await _new_project(client, "Temp")
    assert (await client.delete(f"/api/projects/{pid}")).status_code == 204
    names = {p["name"] for p in (await client.get("/api/projects")).json()["projects"]}
    assert "Temp" not in names
