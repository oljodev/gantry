"""Who may create credits.

Granting credits IS creating money, so this is the security boundary of the
whole billing system: if an ordinary authenticated user can reach it, every
credit limit in Gantry is decorative. The rule is deny-by-default with three
explicit ways through — the internal secret (for the coming payment webhook), an
admin email, or auth being disabled entirely (a single-account dev box).
"""

from __future__ import annotations

import uuid
from decimal import Decimal

import httpx
import pytest
import sqlalchemy as sa
from fastapi import FastAPI
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.billing.users import resolve_user_id
from gantry.core.db import session_scope
from gantry.core.models import Task, TaskStatus, User
from gantry.server.credits_api import ADMIN_SECRET_HEADER

from .test_auth import ALLOWED, FakeAuthenticator

Sessions = async_sessionmaker[AsyncSession]

SECRET = "s3cret-webhook-key"


@pytest.fixture
def authed_app(app: FastAPI) -> FastAPI:
    """Auth ON with no admins named — the shape of a real deployment."""
    app.state.auth = FakeAuthenticator()
    return app


def _bearer(token: str = "good") -> dict[str, str]:
    return {"Authorization": f"Bearer {token}"}


# --- the boundary --------------------------------------------------------


async def test_an_ordinary_authenticated_user_cannot_grant_themselves_credits(
    authed_app: FastAPI, client: httpx.AsyncClient
) -> None:
    """The one that matters: without this, anyone who can log in mints free
    inference for themselves."""
    response = await client.post("/api/credits/grant", json={"credits": 10_000}, headers=_bearer())
    assert response.status_code == 403
    assert "admin" in response.text.lower()


async def test_an_admin_email_may_grant(authed_app: FastAPI, client: httpx.AsyncClient) -> None:
    authed_app.state.settings = authed_app.state.settings.model_copy(
        update={"admin_emails": [ALLOWED]}
    )
    response = await client.post("/api/credits/grant", json={"credits": 250}, headers=_bearer())
    assert response.status_code == 201
    assert response.json()["balance"] >= 250


async def test_the_admin_list_is_matched_case_insensitively(
    authed_app: FastAPI, client: httpx.AsyncClient
) -> None:
    authed_app.state.settings = authed_app.state.settings.model_copy(
        update={"admin_emails": [ALLOWED.upper()]}
    )
    assert (
        await client.post("/api/credits/grant", json={"credits": 5}, headers=_bearer())
    ).status_code == 201


async def test_a_non_admin_stays_refused_even_when_admins_exist(
    authed_app: FastAPI, client: httpx.AsyncClient
) -> None:
    authed_app.state.settings = authed_app.state.settings.model_copy(
        update={"admin_emails": ["someone-else@example.com"]}
    )
    assert (
        await client.post("/api/credits/grant", json={"credits": 5}, headers=_bearer())
    ).status_code == 403


# --- the machine caller (payment webhook) --------------------------------


async def test_the_internal_secret_grants_without_a_user_token(
    authed_app: FastAPI, client: httpx.AsyncClient
) -> None:
    """The Paddle webhook authenticates as itself, with no user session."""
    authed_app.state.settings = authed_app.state.settings.model_copy(
        update={"internal_api_secret": SECRET}
    )
    response = await client.post(
        "/api/credits/grant",
        json={"credits": 500},
        headers={ADMIN_SECRET_HEADER: SECRET},
    )
    assert response.status_code == 201


async def test_a_wrong_secret_is_refused(authed_app: FastAPI, client: httpx.AsyncClient) -> None:
    authed_app.state.settings = authed_app.state.settings.model_copy(
        update={"internal_api_secret": SECRET}
    )
    response = await client.post(
        "/api/credits/grant",
        json={"credits": 500},
        headers={ADMIN_SECRET_HEADER: "not-the-secret"},
    )
    assert response.status_code == 403


async def test_an_unconfigured_secret_cannot_be_matched_by_an_empty_header(
    authed_app: FastAPI, client: httpx.AsyncClient
) -> None:
    """With no secret set, presenting an empty (or any) header must fall through
    to the user check rather than comparing "" == "" and letting anyone in."""
    response = await client.post(
        "/api/credits/grant",
        json={"credits": 500},
        headers={**_bearer(), ADMIN_SECRET_HEADER: ""},
    )
    assert response.status_code == 403


async def test_a_webhook_credits_the_customer_who_paid_not_itself(
    db: Sessions, authed_app: FastAPI, client: httpx.AsyncClient
) -> None:
    authed_app.state.settings = authed_app.state.settings.model_copy(
        update={"internal_api_secret": SECRET}
    )
    async with session_scope(db) as session:
        customer = await resolve_user_id(session, "customer@example.com")
        await session.execute(
            sa.update(User).where(User.id == customer).values(gantry_credits_balance=Decimal(0))
        )

    response = await client.post(
        "/api/credits/grant",
        json={"credits": 900, "user_id": str(customer)},
        headers={ADMIN_SECRET_HEADER: SECRET},
    )
    assert response.status_code == 201
    body = response.json()
    assert body["user_id"] == str(customer)
    assert body["balance"] == 900


async def test_granting_to_an_unknown_account_is_a_404(
    authed_app: FastAPI, client: httpx.AsyncClient
) -> None:
    authed_app.state.settings = authed_app.state.settings.model_copy(
        update={"internal_api_secret": SECRET}
    )
    response = await client.post(
        "/api/credits/grant",
        json={"credits": 10, "user_id": str(uuid.uuid4())},
        headers={ADMIN_SECRET_HEADER: SECRET},
    )
    assert response.status_code == 404


# --- dev parity ----------------------------------------------------------


async def test_auth_disabled_still_allows_a_grant(client: httpx.AsyncClient) -> None:
    """A local box has exactly one account and no way to authenticate as anyone
    else, so there is no privilege to escalate — gating here would only make the
    feature untestable without Supabase."""
    assert (await client.post("/api/credits/grant", json={"credits": 100})).status_code == 201


async def test_a_gated_grant_still_wakes_the_targets_paused_runs(
    db: Sessions, authed_app: FastAPI, client: httpx.AsyncClient
) -> None:
    """Authorization and the resume are one flow: money that lands without
    resuming the work it was bought for leaves a funded, idle swarm."""
    authed_app.state.settings = authed_app.state.settings.model_copy(
        update={"internal_api_secret": SECRET}
    )
    created = await client.post("/api/tasks", json={"goal": "paused"}, headers=_bearer())
    root_id = uuid.UUID(created.json()["id"])
    async with db() as session:
        task = await session.get(Task, root_id)
        assert task is not None
        owner = task.user_id
    assert owner is not None
    async with session_scope(db) as session:
        await session.execute(
            sa.update(Task)
            .where(Task.id == root_id)
            .values(status=TaskStatus.PAUSED_OUT_OF_CREDITS)
        )
        await session.execute(
            sa.update(User).where(User.id == owner).values(gantry_credits_balance=Decimal(0))
        )

    granted = await client.post(
        "/api/credits/grant",
        json={"credits": 750, "user_id": str(owner)},
        headers={ADMIN_SECRET_HEADER: SECRET},
    )
    assert granted.status_code == 201
    async with db() as session:
        resumed = await session.get(Task, root_id)
        assert resumed is not None and resumed.status is TaskStatus.PENDING
