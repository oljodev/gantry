"""Providers API: redaction, vault coupling, delete guards, test probe."""

from __future__ import annotations

import uuid
from typing import Any

import httpx
import pytest
import sqlalchemy as sa
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core.models import DEFAULT_WORKSPACE_ID, AgentProfile, Secret

Sessions = async_sessionmaker[AsyncSession]

KEY = "sk-test-abcdef1234"


async def create_provider(client: httpx.AsyncClient, **overrides: Any) -> dict[str, Any]:
    body: dict[str, Any] = {
        "name": "my-anthropic",
        "provider_type": "anthropic",
        "api_key": KEY,
        "default_model": "claude-opus-4-8",
    }
    body.update(overrides)
    response = await client.post("/api/providers", json=body)
    assert response.status_code == 201, response.text
    return response.json()  # type: ignore[no-any-return]


async def test_create_returns_redacted(client: httpx.AsyncClient) -> None:
    provider = await create_provider(client)
    assert provider["api_key_last4"] == KEY[-4:]
    assert KEY not in str(provider)
    assert "api_key" not in provider


async def test_key_is_encrypted_at_rest(client: httpx.AsyncClient, db: Sessions) -> None:
    provider = await create_provider(client)
    async with db() as session:
        rows = (await session.scalars(sa.select(Secret))).all()
    assert len(rows) == 1
    assert rows[0].name == f"provider:{provider['id']}"
    assert KEY.encode() not in rows[0].ciphertext


async def test_list_never_contains_key(client: httpx.AsyncClient) -> None:
    await create_provider(client)
    response = await client.get("/api/providers")
    assert response.status_code == 200
    assert KEY not in response.text
    providers = response.json()["providers"]
    assert len(providers) == 1
    assert providers[0]["provider_type"] == "anthropic"


async def test_duplicate_name_409(client: httpx.AsyncClient) -> None:
    await create_provider(client)
    response = await client.post(
        "/api/providers",
        json={"name": "my-anthropic", "provider_type": "openai", "default_model": "gpt-5"},
    )
    assert response.status_code == 409


async def test_local_requires_base_url(client: httpx.AsyncClient) -> None:
    response = await client.post(
        "/api/providers",
        json={"name": "ollama", "provider_type": "local", "default_model": "qwen3"},
    )
    assert response.status_code == 422
    ok = await create_provider(
        client,
        name="ollama",
        provider_type="local",
        base_url="http://localhost:11434/v1",
        default_model="qwen3",
    )
    assert ok["base_url"] == "http://localhost:11434/v1"


async def test_delete_removes_secret(client: httpx.AsyncClient, db: Sessions) -> None:
    provider = await create_provider(client)
    response = await client.delete(f"/api/providers/{provider['id']}")
    assert response.status_code == 204
    async with db() as session:
        assert (await session.scalar(sa.select(sa.func.count()).select_from(Secret))) == 0
    assert (await client.get("/api/providers")).json()["providers"] == []


async def test_delete_in_use_409(client: httpx.AsyncClient, db: Sessions) -> None:
    provider = await create_provider(client)
    async with db() as session, session.begin():
        session.add(
            AgentProfile(
                workspace_id=DEFAULT_WORKSPACE_ID,
                name="coder",
                provider_id=uuid.UUID(provider["id"]),
            )
        )
    response = await client.delete(f"/api/providers/{provider['id']}")
    assert response.status_code == 409


async def test_probe_success_and_failure(
    client: httpx.AsyncClient, monkeypatch: pytest.MonkeyPatch
) -> None:
    provider = await create_provider(client, default_model="claude-opus-4-8")
    seen: dict[str, Any] = {}

    async def fake_acompletion(**kwargs: Any) -> Any:
        seen.update(kwargs)
        return object()

    import litellm

    monkeypatch.setattr(litellm, "acompletion", fake_acompletion)
    response = await client.post(f"/api/providers/{provider['id']}/test")
    assert response.status_code == 200
    assert response.json() == {"ok": True, "model": "anthropic/claude-opus-4-8", "error": None}
    # The decrypted key made it to litellm — and only to litellm.
    assert seen["api_key"] == KEY

    async def failing_acompletion(**kwargs: Any) -> Any:
        raise RuntimeError("invalid x-api-key")

    monkeypatch.setattr(litellm, "acompletion", failing_acompletion)
    body = (await client.post(f"/api/providers/{provider['id']}/test")).json()
    assert body["ok"] is False
    assert "invalid x-api-key" in body["error"]


async def test_probe_unknown_provider_404(client: httpx.AsyncClient) -> None:
    response = await client.post("/api/providers/00000000-0000-0000-0000-00000000dead/test")
    assert response.status_code == 404
