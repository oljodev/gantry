"""GitHub integration: store the OAuth token, report status, list repos.

The frontend receives ``provider_token`` from Supabase exactly once (on the
OAuth redirect) and PUTs it here; Supabase never persists it. We validate it
against the GitHub API (which also gives us the login for display), encrypt
it into the vault, and workers use it to clone private repos.
"""

from __future__ import annotations

from typing import Any, cast

import httpx
from fastapi import APIRouter, Depends, HTTPException, Request

from gantry.core.db import session_scope
from gantry.core.models import DEFAULT_WORKSPACE_ID
from gantry.logging import get_logger
from gantry.server.auth import require_user
from gantry.server.providers_api import Sessions, get_vault
from gantry.server.schemas import (
    GithubRepo,
    GithubReposResponse,
    GithubStatusResponse,
    GithubTokenRequest,
)
from gantry.vault.store import GITHUB_TOKEN_SECRET, delete_secret, get_secret, put_secret
from gantry.vault.store import secret_status as get_secret_status

logger = get_logger(__name__)

router = APIRouter(prefix="/api", tags=["github"], dependencies=[Depends(require_user)])

GITHUB_API = "https://api.github.com"
_RECONNECT_HINT = "GitHub token invalid or revoked — sign in with GitHub again to reconnect"


def get_sessions(request: Request) -> Sessions:
    return cast("Sessions", request.app.state.sessions)


def _github_client(request: Request, token: str) -> httpx.AsyncClient:
    """Client for the GitHub API; tests inject a MockTransport via app.state."""
    transport = cast(
        "httpx.AsyncBaseTransport | None", getattr(request.app.state, "github_transport", None)
    )
    return httpx.AsyncClient(
        base_url=GITHUB_API,
        transport=transport,
        timeout=15,
        headers={
            "Authorization": f"Bearer {token}",
            "Accept": "application/vnd.github+json",
            "User-Agent": "gantry",
        },
    )


@router.put("/github/token", response_model=GithubStatusResponse)
async def put_github_token(request: Request, body: GithubTokenRequest) -> GithubStatusResponse:
    vault = get_vault(request)
    async with _github_client(request, body.token) as github:
        response = await github.get("/user")
    if response.status_code == 401:
        raise HTTPException(status_code=422, detail="GitHub rejected this token")
    if response.status_code != 200:
        raise HTTPException(status_code=502, detail=f"GitHub API error {response.status_code}")
    login = str(response.json().get("login", ""))

    sessions = get_sessions(request)
    async with session_scope(sessions) as session:
        hint = await put_secret(
            session,
            vault,
            workspace_id=DEFAULT_WORKSPACE_ID,
            name=GITHUB_TOKEN_SECRET,
            plaintext=body.token,
            meta={"login": login},
        )
    logger.info("github.token_stored", login=login)
    return GithubStatusResponse(connected=True, login=login, last4=hint)


@router.get("/github/status", response_model=GithubStatusResponse)
async def github_status(request: Request) -> GithubStatusResponse:
    """Connection status from stored metadata — never decrypts."""
    sessions = get_sessions(request)
    async with sessions() as session:
        row = await get_secret_status(
            session, workspace_id=DEFAULT_WORKSPACE_ID, name=GITHUB_TOKEN_SECRET
        )
    if row is None:
        return GithubStatusResponse(connected=False)
    return GithubStatusResponse(
        connected=True, login=str(row.meta.get("login") or "") or None, last4=row.last4
    )


@router.delete("/github/token", status_code=204)
async def delete_github_token(request: Request) -> None:
    sessions = get_sessions(request)
    async with session_scope(sessions) as session:
        await delete_secret(session, workspace_id=DEFAULT_WORKSPACE_ID, name=GITHUB_TOKEN_SECRET)


@router.get("/github/repos", response_model=GithubReposResponse)
async def list_github_repos(
    request: Request, page: int = 1, per_page: int = 50
) -> GithubReposResponse:
    vault = get_vault(request)
    sessions = get_sessions(request)
    async with sessions() as session:
        token = await get_secret(
            session, vault, workspace_id=DEFAULT_WORKSPACE_ID, name=GITHUB_TOKEN_SECRET
        )
    if token is None:
        raise HTTPException(status_code=409, detail="no GitHub token stored — connect GitHub first")

    async with _github_client(request, token) as github:
        response = await github.get(
            "/user/repos",
            params={"sort": "pushed", "page": page, "per_page": min(per_page, 100)},
        )
    if response.status_code == 401:
        raise HTTPException(status_code=401, detail=_RECONNECT_HINT)
    if response.status_code != 200:
        raise HTTPException(status_code=502, detail=f"GitHub API error {response.status_code}")

    repos = [
        GithubRepo(
            full_name=str(repo.get("full_name", "")),
            private=bool(repo.get("private", False)),
            default_branch=str(repo.get("default_branch", "main")),
            clone_url=str(repo.get("clone_url", "")),
            pushed_at=repo.get("pushed_at"),
        )
        for repo in cast("list[dict[str, Any]]", response.json())
    ]
    return GithubReposResponse(repos=repos)
