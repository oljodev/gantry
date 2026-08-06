"""The credits surface end-to-end: launching attributes a run to an account,
children inherit it, and the balance/run endpoints report what was charged.
"""

from __future__ import annotations

import uuid
from decimal import Decimal
from typing import Any

import httpx
import sqlalchemy as sa
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.billing.catalog import CATALOG, TokenPrice
from gantry.billing.ledger import BillingContext, record_call_in_session
from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.models import DEFAULT_WORKSPACE_ID, LOCAL_USER_ID, Task, TaskKind
from gantry.runtime.llm import LLMUsage

Sessions = async_sessionmaker[AsyncSession]

TEST_MODEL = "testvendor/fixture-model"
TEST_PRICE = TokenPrice(Decimal(10), Decimal(30), Decimal(1))


async def test_launching_a_task_attributes_it_to_the_caller(client: httpx.AsyncClient) -> None:
    created = await client.post("/api/tasks", json={"goal": "do the thing"})
    assert created.status_code == 201, created.text

    balance = await client.get("/api/credits")
    assert balance.status_code == 200
    # Auth is disabled in tests, so the run bills the seeded local account —
    # metering must behave the same with and without Supabase.
    assert balance.json()["user_id"] == str(LOCAL_USER_ID)


async def test_a_spawned_child_inherits_the_owner(db: Sessions, client: httpx.AsyncClient) -> None:
    """A swarm bills ONE account however deep it fans out."""
    created = await client.post("/api/tasks", json={"goal": "lead"})
    root_id = uuid.UUID(created.json()["id"])

    async with session_scope(db) as session:
        parent = await session.get(Task, root_id)
        assert parent is not None
        child = await queue.enqueue(
            session,
            workspace_id=DEFAULT_WORKSPACE_ID,
            kind=TaskKind.EXECUTE,
            payload={"goal": "child work"},
            parent=parent,
        )
        assert child.user_id == parent.user_id
        assert child.user_id is not None


async def test_the_balance_endpoint_reports_spend_and_the_pricing_basis(
    db: Sessions, client: httpx.AsyncClient
) -> None:
    CATALOG.load({TEST_MODEL: TEST_PRICE})
    before: dict[str, Any] = (await client.get("/api/credits")).json()

    await record_call_in_session(
        db,
        BillingContext(workspace_id=DEFAULT_WORKSPACE_ID, user_id=uuid.UUID(before["user_id"])),
        model_slug=TEST_MODEL,
        usage=LLMUsage(prompt_tokens=100_000, completion_tokens=10_000),
    )

    after: dict[str, Any] = (await client.get("/api/credits")).json()
    charged = 1.30 / 0.60 * 100  # $1.30 raw, 40% margin, 100 GC/USD
    assert after["lifetime_credits_used"] == round(charged, 6)
    assert after["balance"] == round(before["balance"] - charged, 6)
    assert after["calls"] == 1
    # The UI states the pricing basis from the server, never a hard-coded copy.
    assert after["target_margin"] == 0.4
    assert after["credits_per_usd"] == 100.0


async def test_run_credits_accumulate_across_the_whole_tree(
    db: Sessions, client: httpx.AsyncClient
) -> None:
    """Answerable DURING execution: rows land as each call returns, so a live
    swarm's spend is visible without waiting for its tasks to settle."""
    CATALOG.load({TEST_MODEL: TEST_PRICE})
    run_id = uuid.uuid4()
    user_id = uuid.UUID((await client.get("/api/credits")).json()["user_id"])
    ctx = BillingContext(workspace_id=DEFAULT_WORKSPACE_ID, user_id=user_id, run_id=run_id)

    for _ in range(3):
        await record_call_in_session(
            db, ctx, model_slug=TEST_MODEL, usage=LLMUsage(prompt_tokens=1000, completion_tokens=0)
        )

    body = (await client.get(f"/api/credits/runs/{run_id}")).json()
    assert body["calls"] == 3
    assert body["prompt_tokens"] == 3000
    assert body["credits_used"] > 0
    assert body["raw_cost_usd"] > 0


async def test_run_credits_are_zero_for_a_run_that_has_not_spent(
    client: httpx.AsyncClient,
) -> None:
    body = (await client.get(f"/api/credits/runs/{uuid.uuid4()}")).json()
    assert body["credits_used"] == 0
    assert body["calls"] == 0


async def test_the_usage_log_exposes_the_audit_trail_and_a_per_model_rollup(
    db: Sessions, client: httpx.AsyncClient
) -> None:
    CATALOG.load(
        {TEST_MODEL: TEST_PRICE, "other/model": TokenPrice(Decimal(1), Decimal(1), Decimal(1))}
    )
    run_id = uuid.uuid4()
    user_id = uuid.UUID((await client.get("/api/credits")).json()["user_id"])
    ctx = BillingContext(workspace_id=DEFAULT_WORKSPACE_ID, user_id=user_id, run_id=run_id)
    await record_call_in_session(
        db, ctx, model_slug=TEST_MODEL, usage=LLMUsage(prompt_tokens=100_000, completion_tokens=0)
    )
    await record_call_in_session(
        db, ctx, model_slug="other/model", usage=LLMUsage(prompt_tokens=1000, completion_tokens=0)
    )

    body = (await client.get(f"/api/credits/usage?run_id={run_id}")).json()
    assert len(body["entries"]) == 2
    # Both the raw cost and the credit charge ride along — the ratio between them
    # IS the realised margin, which is the whole reason both are stored.
    assert all(e["raw_cost_usd"] > 0 and e["credits_deducted"] > 0 for e in body["entries"])
    models = {m["model_slug"]: m for m in body["by_model"]}
    assert set(models) == {TEST_MODEL, "other/model"}
    # Ordered by spend, so the expensive model leads.
    assert body["by_model"][0]["model_slug"] == TEST_MODEL


async def test_granting_credits_raises_the_balance(client: httpx.AsyncClient) -> None:
    before = (await client.get("/api/credits")).json()["balance"]
    granted = await client.post("/api/credits/grant", json={"credits": 250})
    assert granted.status_code == 201
    assert granted.json()["balance"] == before + 250


async def test_granting_a_non_positive_amount_is_rejected(client: httpx.AsyncClient) -> None:
    assert (await client.post("/api/credits/grant", json={"credits": 0})).status_code == 422
    assert (await client.post("/api/credits/grant", json={"credits": -5})).status_code == 422


async def test_a_deleted_account_does_not_take_its_audit_trail_with_it(
    db: Sessions, client: httpx.AsyncClient
) -> None:
    """Billing records outlive the account: an FK cascade here would erase the
    evidence behind money that was actually spent."""
    CATALOG.load({TEST_MODEL: TEST_PRICE})
    user_id = uuid.UUID((await client.get("/api/credits")).json()["user_id"])
    await record_call_in_session(
        db,
        BillingContext(workspace_id=DEFAULT_WORKSPACE_ID, user_id=user_id),
        model_slug=TEST_MODEL,
        usage=LLMUsage(prompt_tokens=1000, completion_tokens=0),
    )
    async with session_scope(db) as session:
        await session.execute(sa.text("DELETE FROM users WHERE id = :i").bindparams(i=user_id))

    body = (await client.get("/api/credits/usage")).json()
    assert len(body["entries"]) == 1
    assert body["entries"][0]["model_slug"] == TEST_MODEL
