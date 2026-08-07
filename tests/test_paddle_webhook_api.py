"""``POST /api/webhooks/paddle`` end-to-end: real HTTP request, real signature,
real database. Where test_paddle_signature.py proves the pure functions are
correct in isolation, this proves they are actually wired into the route —
that a valid, signed ``transaction.completed`` really does move money and wake
a paused run, and that a bad signature never gets that far.
"""

from __future__ import annotations

import json
import time
import uuid
from decimal import Decimal
from typing import Any

import httpx
import pytest
import sqlalchemy as sa
from fastapi import FastAPI
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core.db import session_scope
from gantry.core.models import PaddleWebhookEvent, Task, TaskStatus, User

from .test_paddle_signature import sign

Sessions = async_sessionmaker[AsyncSession]

SECRET = "pdl_ntfset_test_secret"


@pytest.fixture
def paddle_app(app: FastAPI) -> FastAPI:
    """The webhook secret configured — the shape of a real deployment."""
    app.state.settings = app.state.settings.model_copy(
        update={"paddle_webhook_secret": SECRET, "credits_per_usd": 100.0}
    )
    return app


def _signed(body: dict[str, Any]) -> tuple[bytes, dict[str, str]]:
    raw = json.dumps(body).encode()
    # The production route verifies against real wall-clock time (it never
    # passes `now=`), so a fixed test timestamp would fail the freshness check
    # regardless of when the suite runs — sign as "right now" instead.
    header = sign(raw, secret=SECRET, ts=int(time.time()))
    return raw, {"Paddle-Signature": header}


async def _current_user(client: httpx.AsyncClient) -> uuid.UUID:
    return uuid.UUID((await client.get("/api/credits")).json()["user_id"])


async def _balance(db: Sessions, user_id: uuid.UUID) -> Decimal:
    async with db() as session:
        value = await session.scalar(
            sa.select(User.gantry_credits_balance).where(User.id == user_id)
        )
    assert value is not None
    return Decimal(value)


# --- signature enforcement, over real HTTP --------------------------------


async def test_a_request_with_no_signature_header_is_rejected(
    paddle_app: FastAPI, client: httpx.AsyncClient
) -> None:
    response = await client.post("/api/webhooks/paddle", content=b"{}")
    assert response.status_code == 401


async def test_a_request_with_a_wrong_signature_is_rejected(
    paddle_app: FastAPI, client: httpx.AsyncClient
) -> None:
    response = await client.post(
        "/api/webhooks/paddle",
        content=b'{"event_id": "evt_1"}',
        headers={"Paddle-Signature": "ts=1700000000;h1=deadbeef"},
    )
    assert response.status_code == 401


async def test_an_unconfigured_secret_refuses_everything(
    app: FastAPI, client: httpx.AsyncClient
) -> None:
    """The base `app` fixture (no paddle_app override) never sets a webhook
    secret — must fail closed, not fall through to accepting unsigned bodies."""
    response = await client.post("/api/webhooks/paddle", content=b"{}")
    assert response.status_code == 401


async def test_malformed_json_with_a_valid_signature_is_a_400(
    paddle_app: FastAPI, client: httpx.AsyncClient
) -> None:
    raw = b"not json at all"
    response = await client.post(
        "/api/webhooks/paddle",
        content=raw,
        headers={"Paddle-Signature": sign(raw, secret=SECRET, ts=int(time.time()))},
    )
    assert response.status_code == 400


# --- the real path: grant + resume -----------------------------------------


async def test_a_valid_transaction_completed_grants_credits_and_resumes_a_paused_run(
    db: Sessions, paddle_app: FastAPI, client: httpx.AsyncClient
) -> None:
    created = await client.post("/api/tasks", json={"goal": "paused run"})
    root_id = uuid.UUID(created.json()["id"])
    user_id = await _current_user(client)
    before = await _balance(db, user_id)

    async with session_scope(db) as session:
        await session.execute(
            sa.update(Task)
            .where(Task.id == root_id)
            .values(status=TaskStatus.PAUSED_OUT_OF_CREDITS)
        )

    body = {
        "event_id": "evt_txn_1",
        "event_type": "transaction.completed",
        "data": {
            "custom_data": {"user_id": str(user_id)},
            "items": [{"price": {"id": "pri_unmapped"}}],
            "details": {"totals": {"grand_total": "999", "currency_code": "USD"}},
        },
    }
    raw, headers = _signed(body)
    response = await client.post("/api/webhooks/paddle", content=raw, headers=headers)

    assert response.status_code == 200, response.text
    payload = response.json()
    assert payload["status"] == "granted"
    assert payload["credits_granted"] == pytest.approx(999.0)
    assert payload["resumed_tasks"] == 1

    assert await _balance(db, user_id) == before + Decimal("999.000000")
    async with db() as session:
        task = await session.get(Task, root_id)
        assert task is not None and task.status is TaskStatus.PENDING


async def test_subscription_created_grants_using_the_configured_price_map(
    db: Sessions, paddle_app: FastAPI, client: httpx.AsyncClient
) -> None:
    paddle_app.state.settings = paddle_app.state.settings.model_copy(
        update={"paddle_price_credits": {"pri_monthly_5000": 5000.0}}
    )
    user_id = await _current_user(client)
    before = await _balance(db, user_id)

    body = {
        "event_id": "evt_sub_1",
        "event_type": "subscription.created",
        "data": {
            "custom_data": {"user_id": str(user_id)},
            "items": [{"price": {"id": "pri_monthly_5000"}}],
        },
    }
    raw, headers = _signed(body)
    response = await client.post("/api/webhooks/paddle", content=raw, headers=headers)

    assert response.status_code == 200
    assert response.json()["credits_granted"] == 5000.0
    assert await _balance(db, user_id) == before + Decimal(5000)


async def test_a_redelivered_event_is_not_granted_twice(
    db: Sessions, paddle_app: FastAPI, client: httpx.AsyncClient
) -> None:
    """Paddle retries on anything but a prompt 2xx and can legitimately
    redeliver the same event — this is the financial-correctness test."""
    user_id = await _current_user(client)
    before = await _balance(db, user_id)
    body = {
        "event_id": "evt_replay",
        "event_type": "transaction.completed",
        "data": {
            "custom_data": {"user_id": str(user_id)},
            "items": [{"price": {"id": "pri_x"}}],
            "details": {"totals": {"grand_total": "500", "currency_code": "USD"}},
        },
    }
    raw, headers = _signed(body)

    first = await client.post("/api/webhooks/paddle", content=raw, headers=headers)
    assert first.json()["status"] == "granted"
    after_first = await _balance(db, user_id)
    assert after_first == before + Decimal("500.000000")

    second = await client.post("/api/webhooks/paddle", content=raw, headers=headers)
    assert second.status_code == 200
    assert second.json()["status"] == "duplicate"
    # The balance must not move a second time.
    assert await _balance(db, user_id) == after_first


async def test_the_idempotency_row_records_the_event(
    db: Sessions, paddle_app: FastAPI, client: httpx.AsyncClient
) -> None:
    user_id = await _current_user(client)
    body = {
        "event_id": "evt_recorded",
        "event_type": "transaction.completed",
        "data": {
            "custom_data": {"user_id": str(user_id)},
            "details": {"totals": {"grand_total": "250", "currency_code": "USD"}},
        },
    }
    raw, headers = _signed(body)
    await client.post("/api/webhooks/paddle", content=raw, headers=headers)

    async with db() as session:
        row = await session.get(PaddleWebhookEvent, "evt_recorded")
    assert row is not None
    assert row.event_type == "transaction.completed"
    assert row.user_id == user_id
    assert row.credits_granted == Decimal("250.000000")


# --- non-actionable but still-valid deliveries -----------------------------


async def test_a_non_granting_event_type_is_acknowledged_without_granting(
    db: Sessions, paddle_app: FastAPI, client: httpx.AsyncClient
) -> None:
    user_id = await _current_user(client)
    before = await _balance(db, user_id)
    body = {
        "event_id": "evt_other",
        "event_type": "subscription.updated",
        "data": {"custom_data": {"user_id": str(user_id)}},
    }
    raw, headers = _signed(body)
    response = await client.post("/api/webhooks/paddle", content=raw, headers=headers)

    assert response.status_code == 200
    assert response.json()["status"] == "skipped"
    assert await _balance(db, user_id) == before


async def test_an_unknown_user_id_is_skipped_not_crashed(
    paddle_app: FastAPI, client: httpx.AsyncClient
) -> None:
    body = {
        "event_id": "evt_ghost",
        "event_type": "transaction.completed",
        "data": {
            "custom_data": {"user_id": str(uuid.uuid4())},
            "details": {"totals": {"grand_total": "100", "currency_code": "USD"}},
        },
    }
    raw, headers = _signed(body)
    response = await client.post("/api/webhooks/paddle", content=raw, headers=headers)

    assert response.status_code == 200
    assert response.json()["status"] == "skipped"


async def test_a_non_uuid_custom_data_user_id_is_skipped_not_500(
    paddle_app: FastAPI, client: httpx.AsyncClient
) -> None:
    body = {
        "event_id": "evt_bad_id",
        "event_type": "transaction.completed",
        "data": {
            "custom_data": {"user_id": "not-a-uuid"},
            "details": {"totals": {"grand_total": "100", "currency_code": "USD"}},
        },
    }
    raw, headers = _signed(body)
    response = await client.post("/api/webhooks/paddle", content=raw, headers=headers)

    assert response.status_code == 200
    assert response.json()["status"] == "skipped"
