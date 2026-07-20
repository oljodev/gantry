"""Auth: optional-by-default, bearer enforcement, WS tokens, /api/me, JWT verify."""

from __future__ import annotations

import time
import uuid
from typing import Any

import httpx
import jwt
import pytest
from cryptography.hazmat.primitives.asymmetric.ec import SECP256R1, generate_private_key
from fastapi import FastAPI
from httpx_ws import WebSocketDisconnect

from gantry.server.auth import AuthContext, AuthFailed, SupabaseAuthenticator

from .test_api_ws import connect, recv

ALLOWED = "olav@example.com"


class FakeAuthenticator:
    """Accepts exactly one token; mirrors SupabaseAuthenticator's contract."""

    def __init__(self, valid_token: str = "good", email: str = ALLOWED) -> None:
        self.valid_token = valid_token
        self.email = email

    async def authenticate(self, token: str) -> AuthContext:
        if token == self.valid_token:
            return AuthContext(email=self.email, subject="user-1")
        if token == "wrong-account":
            raise AuthFailed(403, "not allowed", email="intruder@example.com")
        raise AuthFailed(401, "invalid token")


@pytest.fixture
def authed_app(app: FastAPI) -> FastAPI:
    app.state.auth = FakeAuthenticator()
    return app


class TestAuthDisabled:
    """Regression guard: without GANTRY_SUPABASE_URL everything stays open."""

    async def test_routes_open(self, client: httpx.AsyncClient) -> None:
        assert (await client.get("/api/tasks")).status_code == 200

    async def test_me_reports_disabled(self, client: httpx.AsyncClient) -> None:
        body = (await client.get("/api/me")).json()
        assert body == {"auth_enabled": False, "email": None, "allowed": True}


class TestAuthEnabled:
    async def test_missing_token_401(self, authed_app: FastAPI, client: httpx.AsyncClient) -> None:
        assert (await client.get("/api/tasks")).status_code == 401

    async def test_garbage_token_401(self, authed_app: FastAPI, client: httpx.AsyncClient) -> None:
        response = await client.get("/api/tasks", headers={"Authorization": "Bearer nonsense"})
        assert response.status_code == 401

    async def test_disallowed_email_403(
        self, authed_app: FastAPI, client: httpx.AsyncClient
    ) -> None:
        response = await client.get("/api/tasks", headers={"Authorization": "Bearer wrong-account"})
        assert response.status_code == 403

    async def test_valid_token_200(self, authed_app: FastAPI, client: httpx.AsyncClient) -> None:
        response = await client.get("/api/tasks", headers={"Authorization": "Bearer good"})
        assert response.status_code == 200

    async def test_me_three_states(self, authed_app: FastAPI, client: httpx.AsyncClient) -> None:
        anonymous = (await client.get("/api/me")).json()
        assert anonymous == {"auth_enabled": True, "email": None, "allowed": False}
        valid = (await client.get("/api/me", headers={"Authorization": "Bearer good"})).json()
        assert valid == {"auth_enabled": True, "email": ALLOWED, "allowed": True}
        rejected = (
            await client.get("/api/me", headers={"Authorization": "Bearer wrong-account"})
        ).json()
        assert rejected["allowed"] is False
        assert rejected["email"] == "intruder@example.com"

    async def test_ws_requires_token(self, authed_app: FastAPI, client: httpx.AsyncClient) -> None:
        async with connect(client, "/api/events/ws") as ws:
            with pytest.raises(WebSocketDisconnect) as excinfo:
                await recv(ws, timeout_seconds=5)
            assert excinfo.value.code == 4401

    async def test_ws_accepts_token(self, authed_app: FastAPI, client: httpx.AsyncClient) -> None:
        async with connect(client, "/api/events/ws?token=good") as ws:
            # No close means we are subscribed; just ensure the socket survives
            # a moment without the server hanging up.
            with pytest.raises(TimeoutError):
                await recv(ws, timeout_seconds=0.3)

    async def test_ws_rejects_bad_token(
        self, authed_app: FastAPI, client: httpx.AsyncClient
    ) -> None:
        task_id = uuid.uuid4()
        async with connect(client, f"/api/tasks/{task_id}/events/ws?token=bad") as ws:
            with pytest.raises(WebSocketDisconnect) as excinfo:
                await recv(ws, timeout_seconds=5)
            assert excinfo.value.code == 4401


class TestSupabaseAuthenticator:
    """Real JWT verification against a locally generated ES256 keypair."""

    SUPABASE_URL = "https://project.supabase.co"

    def _make(
        self, monkeypatch: pytest.MonkeyPatch, allowed: list[str]
    ) -> tuple[SupabaseAuthenticator, Any]:
        private_key = generate_private_key(SECP256R1())
        authenticator = SupabaseAuthenticator(self.SUPABASE_URL, allowed_emails=allowed)

        class FakeSigningKey:
            key = private_key.public_key()

        monkeypatch.setattr(
            authenticator._jwks, "get_signing_key_from_jwt", lambda _token: FakeSigningKey()
        )
        return authenticator, private_key

    def _token(self, private_key: Any, **overrides: Any) -> str:
        claims: dict[str, Any] = {
            "sub": "user-123",
            "email": ALLOWED,
            "aud": "authenticated",
            "iss": f"{self.SUPABASE_URL}/auth/v1",
            "exp": int(time.time()) + 3600,
        }
        claims.update(overrides)
        return jwt.encode(claims, private_key, algorithm="ES256")

    async def test_valid_es256_token(self, monkeypatch: pytest.MonkeyPatch) -> None:
        authenticator, key = self._make(monkeypatch, allowed=[ALLOWED])
        ctx = await authenticator.authenticate(self._token(key))
        assert ctx == AuthContext(email=ALLOWED, subject="user-123")

    async def test_allowlist_case_insensitive(self, monkeypatch: pytest.MonkeyPatch) -> None:
        authenticator, key = self._make(monkeypatch, allowed=["OLAV@Example.COM"])
        ctx = await authenticator.authenticate(self._token(key))
        assert ctx.email == ALLOWED

    async def test_rejects_unlisted_email(self, monkeypatch: pytest.MonkeyPatch) -> None:
        authenticator, key = self._make(monkeypatch, allowed=[ALLOWED])
        with pytest.raises(AuthFailed) as excinfo:
            await authenticator.authenticate(self._token(key, email="evil@example.com"))
        assert excinfo.value.status_code == 403
        assert excinfo.value.email == "evil@example.com"

    async def test_rejects_expired(self, monkeypatch: pytest.MonkeyPatch) -> None:
        authenticator, key = self._make(monkeypatch, allowed=[ALLOWED])
        with pytest.raises(AuthFailed) as excinfo:
            await authenticator.authenticate(self._token(key, exp=int(time.time()) - 3600))
        assert excinfo.value.status_code == 401

    async def test_rejects_wrong_issuer(self, monkeypatch: pytest.MonkeyPatch) -> None:
        authenticator, key = self._make(monkeypatch, allowed=[ALLOWED])
        with pytest.raises(AuthFailed):
            await authenticator.authenticate(
                self._token(key, iss="https://other.supabase.co/auth/v1")
            )

    async def test_rejects_garbage(self, monkeypatch: pytest.MonkeyPatch) -> None:
        authenticator, _ = self._make(monkeypatch, allowed=[ALLOWED])
        with pytest.raises(AuthFailed):
            await authenticator.authenticate("not-a-jwt")

    async def test_empty_allowlist_admits_any_valid_token(
        self, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        authenticator, key = self._make(monkeypatch, allowed=[])
        ctx = await authenticator.authenticate(self._token(key, email="anyone@example.com"))
        assert ctx.email == "anyone@example.com"
