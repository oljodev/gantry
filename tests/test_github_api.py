"""GitHub API: token store/status/delete roundtrip and the repos proxy."""

from __future__ import annotations

from typing import Any

import httpx
from fastapi import FastAPI

GOOD_TOKEN = "gho_valid_token_abcd"


def install_github_mock(app: FastAPI) -> None:
    """Mock GitHub: /user and /user/repos honoring only GOOD_TOKEN."""

    def handler(request: httpx.Request) -> httpx.Response:
        if request.headers.get("Authorization") != f"Bearer {GOOD_TOKEN}":
            return httpx.Response(401, json={"message": "Bad credentials"})
        if request.url.path == "/user":
            return httpx.Response(200, json={"login": "oljodev"})
        if request.url.path == "/user/repos":
            return httpx.Response(
                200,
                json=[
                    {
                        "full_name": "oljodev/gantry",
                        "private": False,
                        "default_branch": "main",
                        "clone_url": "https://github.com/oljodev/gantry.git",
                        "pushed_at": "2026-07-20T10:00:00Z",
                    },
                    {
                        "full_name": "oljodev/secret-sauce",
                        "private": True,
                        "default_branch": "master",
                        "clone_url": "https://github.com/oljodev/secret-sauce.git",
                        "pushed_at": None,
                    },
                ],
            )
        return httpx.Response(404, json={})

    app.state.github_transport = httpx.MockTransport(handler)


async def test_token_roundtrip(app: FastAPI, client: httpx.AsyncClient) -> None:
    install_github_mock(app)

    before = (await client.get("/api/github/status")).json()
    assert before == {"connected": False, "login": None, "last4": None}

    stored = await client.put("/api/github/token", json={"token": GOOD_TOKEN})
    assert stored.status_code == 200
    assert stored.json() == {"connected": True, "login": "oljodev", "last4": GOOD_TOKEN[-4:]}

    status = (await client.get("/api/github/status")).json()
    assert status == {"connected": True, "login": "oljodev", "last4": GOOD_TOKEN[-4:]}

    assert (await client.delete("/api/github/token")).status_code == 204
    assert (await client.get("/api/github/status")).json()["connected"] is False


async def test_put_rejects_bad_token(app: FastAPI, client: httpx.AsyncClient) -> None:
    install_github_mock(app)
    response = await client.put("/api/github/token", json={"token": "gho_wrong"})
    assert response.status_code == 422
    assert (await client.get("/api/github/status")).json()["connected"] is False


async def test_repos_proxy(app: FastAPI, client: httpx.AsyncClient) -> None:
    install_github_mock(app)
    await client.put("/api/github/token", json={"token": GOOD_TOKEN})

    repos: list[dict[str, Any]] = (await client.get("/api/github/repos")).json()["repos"]
    assert [r["full_name"] for r in repos] == ["oljodev/gantry", "oljodev/secret-sauce"]
    assert repos[1]["private"] is True
    assert repos[1]["default_branch"] == "master"


async def test_repos_without_token_409(app: FastAPI, client: httpx.AsyncClient) -> None:
    install_github_mock(app)
    assert (await client.get("/api/github/repos")).status_code == 409


async def test_repos_revoked_token_maps_to_401(app: FastAPI, client: httpx.AsyncClient) -> None:
    install_github_mock(app)
    await client.put("/api/github/token", json={"token": GOOD_TOKEN})

    # Simulate revocation: GitHub now rejects every token.
    def reject_all(request: httpx.Request) -> httpx.Response:
        return httpx.Response(401, json={"message": "Bad credentials"})

    app.state.github_transport = httpx.MockTransport(reject_all)
    response = await client.get("/api/github/repos")
    assert response.status_code == 401
    assert "sign in with GitHub again" in response.json()["detail"]


async def test_token_never_appears_in_responses(app: FastAPI, client: httpx.AsyncClient) -> None:
    install_github_mock(app)
    await client.put("/api/github/token", json={"token": GOOD_TOKEN})
    for path in ("/api/github/status", "/api/github/repos"):
        body = (await client.get(path)).text
        assert GOOD_TOKEN not in body
