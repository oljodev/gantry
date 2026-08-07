"""One end-to-end walk through the full stack, driven the way a real run is:
launched over HTTP with an attached screenshot, claimed and run by a real
``Worker``, metered call-by-call, paused the moment its account runs dry, and
resumed by an actual signed Paddle webhook hitting the real route.

Every other test file proves one layer in isolation (attachments, credits,
pause/resume, Paddle) against a scripted stand-in for its neighbours. This is
the one place all of them are wired together at once, so a regression that only
shows up at the seam between two subsystems — attachments billed through the
wrong client, a pause that resumes into a different conversation, a webhook
grant that doesn't actually wake the run it funded — has somewhere to be
caught.
"""

from __future__ import annotations

import json
import time
import uuid
from decimal import Decimal
from pathlib import Path

import httpx
import sqlalchemy as sa
from fastapi import FastAPI
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.attachments.storage import AttachmentStore, LocalAttachmentStore
from gantry.billing.catalog import CATALOG, TokenPrice
from gantry.billing.credits import calculate_credits_for_call
from gantry.billing.ledger import credit_balance
from gantry.core.db import session_scope
from gantry.core.models import Attachment, LlmUsageLog, Task, TaskStatus, User
from gantry.worker.service import Worker, WorkerConfig

from .fakes import ScriptedLLM, final_response, response_with_tool_call
from .test_paddle_signature import sign
from .test_worker_service import claim_as, get_task

Sessions = async_sessionmaker[AsyncSession]

#: A real (tiny) PNG, so the upload sniffs as an image and the vision pre-pass
#: actually has a modality to route.
PNG = b"\x89PNG\r\n\x1a\n" + b"\x00" * 64

#: "deepseek" in the slug -> TEXT_ONLY (capabilities.py), so the attachment
#: cannot be read directly and must go through the vision pre-pass.
MODEL = "fake/e2e-deepseek-coder"
#: "claude" in the slug -> VISION_PDF, i.e. can read the image the worker can't.
VISION_MODEL = "fake/e2e-claude-vision"
#: Priced so a single 10-prompt/5-completion call costs a large, non-trivial
#: number of credits — the balance can be sized in whole calls, not fractions.
EXPENSIVE = TokenPrice(Decimal(100_000), Decimal(300_000), Decimal(10_000))

PADDLE_SECRET = "e2e-paddle-secret"
TOPUP_CREDITS = Decimal(10_000)


async def _current_user(client: httpx.AsyncClient) -> uuid.UUID:
    return uuid.UUID((await client.get("/api/credits")).json()["user_id"])


def _worker(
    db: Sessions,
    tmp_path: Path,
    llm: ScriptedLLM,
    *,
    attachment_store: AttachmentStore | None = None,
) -> Worker:
    config = WorkerConfig(
        worker_id="svc-worker-1",
        workspace_root=tmp_path / "workspaces",
        poll_interval_seconds=0.05,
        enforce_credit_balance=True,
        vision_model=VISION_MODEL,
    )
    return Worker(db, config, llm, attachment_store=attachment_store)


async def test_a_multi_step_run_with_an_attachment_pauses_on_zero_credit_and_resumes_via_paddle(
    db: Sessions, app: FastAPI, client: httpx.AsyncClient, tmp_path: Path
) -> None:
    # --- launch: upload an image, attach it to a real task over HTTP --------
    uploaded = (
        await client.post("/api/attachments", files={"file": ("screenshot.png", PNG, "image/png")})
    ).json()
    assert uploaded["kind"] == "image"

    created = await client.post(
        "/api/tasks",
        json={
            "goal": "Describe the attached screenshot and propose a matching team.",
            "model": MODEL,
            "attachment_ids": [uploaded["id"]],
            "payload": {"copilot": "tree"},
        },
    )
    assert created.status_code == 201, created.text
    task_id = uuid.UUID(created.json()["id"])
    user_id = await _current_user(client)

    # The worker reads attachment bytes from the same blob store the API wrote
    # them to — a store built fresh here would just 404 the file.
    store = LocalAttachmentStore(Path(app.state.settings.attachment_root))

    # Size the balance in whole calls: it must survive the vision pre-pass call
    # plus one agent step, then run out exactly at the next step boundary.
    CATALOG.load({MODEL: EXPENSIVE, VISION_MODEL: EXPENSIVE})
    charge = calculate_credits_for_call(MODEL, 10, 5).credits_deducted
    assert charge > 0
    async with session_scope(db) as session:
        await session.execute(
            sa.update(User).where(User.id == user_id).values(gantry_credits_balance=charge * 2)
        )

    # --- attempt 1: vision pre-pass + one real agent step, then pause -------
    first_worker = _worker(
        db,
        tmp_path,
        ScriptedLLM(
            [
                final_response("A centered login card with a blue Sign In button."),
                response_with_tool_call(
                    "call-1", "propose_tree", {"name": "Team", "root": {"name": "root"}}
                ),
            ]
        ),
        attachment_store=store,
    )
    claimed = await claim_as(db, first_worker)
    assert claimed.id == task_id
    await first_worker.process(claimed)

    paused = await get_task(db, claimed)
    assert paused.status is TaskStatus.PAUSED_OUT_OF_CREDITS

    # The image really went through the vision pre-pass, and the transcript was
    # persisted on the attachment row (not just held in memory for this attempt).
    async with db() as session:
        attachment = await session.get(Attachment, uuid.UUID(uploaded["id"]))
    assert attachment is not None
    assert attachment.transcript is not None
    assert "login card" in attachment.transcript
    assert attachment.transcript_model == VISION_MODEL

    # Both calls (the pre-pass AND the agent step) left a real usage row and
    # drew the balance down to exactly zero — no charge happened off-ledger.
    async with db() as session:
        rows = list(
            (
                await session.scalars(sa.select(LlmUsageLog).where(LlmUsageLog.user_id == user_id))
            ).all()
        )
        balance = await credit_balance(session, user_id)
    assert {r.model_slug for r in rows} == {MODEL, VISION_MODEL}
    assert all(r.run_id == task_id for r in rows)
    assert balance == Decimal(0)

    # --- top-up: a real signed Paddle webhook, not a direct DB grant --------
    app.state.settings = app.state.settings.model_copy(
        update={
            "paddle_webhook_secret": PADDLE_SECRET,
            "paddle_price_credits": {"pri_e2e": 10_000.0},
        }
    )
    body = {
        "event_id": "evt_e2e_topup",
        "event_type": "transaction.completed",
        "data": {
            "custom_data": {"user_id": str(user_id)},
            "items": [{"price": {"id": "pri_e2e"}}],
        },
    }
    raw = json.dumps(body).encode()
    webhook = await client.post(
        "/api/webhooks/paddle",
        content=raw,
        headers={"Paddle-Signature": sign(raw, secret=PADDLE_SECRET, ts=int(time.time()))},
    )
    assert webhook.status_code == 200, webhook.text
    outcome = webhook.json()
    assert outcome["status"] == "granted"
    assert outcome["credits_granted"] == float(TOPUP_CREDITS)
    assert outcome["resumed_tasks"] == 1

    resumed = await get_task(db, claimed)
    assert resumed.status is TaskStatus.PENDING

    # --- attempt 2: the swarm actually finishes, continuing the SAME log ----
    # No vision call is scripted here: the transcript was already persisted on
    # the attachment row, so this attempt must reuse it rather than paying for
    # (and needing) a second transcription.
    second_worker = _worker(
        db,
        tmp_path,
        ScriptedLLM([final_response("finished after the Paddle top-up")]),
        attachment_store=store,
    )
    reclaimed = await claim_as(db, second_worker)
    assert reclaimed.id == task_id
    await second_worker.process(reclaimed)

    settled = await get_task(db, claimed)
    assert settled.status is TaskStatus.SUCCEEDED
    assert settled.result is not None
    assert "finished after the Paddle top-up" in str(settled.result.get("final_text", ""))

    async with db() as session:
        final_rows = list(
            (
                await session.scalars(sa.select(LlmUsageLog).where(LlmUsageLog.user_id == user_id))
            ).all()
        )
        final_balance = await credit_balance(session, user_id)
    # Exactly one more call happened (the finishing step) — the cached
    # transcript means the pre-pass did not run, and did not bill, again.
    assert len(final_rows) == len(rows) + 1
    assert final_balance == TOPUP_CREDITS - charge


async def test_an_owned_task_launched_via_the_api_is_billed_and_completes(
    db: Sessions, client: httpx.AsyncClient, tmp_path: Path
) -> None:
    """A plainer companion to the pause/resume test above: no attachment, no
    pause, just proof that a task launched through the real API — not
    hand-built with `queue.enqueue` — is billed to the account that owns it."""
    CATALOG.load({MODEL: TokenPrice(Decimal(1_000), Decimal(3_000), Decimal(100))})
    created = await client.post(
        "/api/tasks", json={"goal": "plain run", "model": MODEL, "payload": {"copilot": "tree"}}
    )
    assert created.status_code == 201, created.text
    task_id = uuid.UUID(created.json()["id"])
    user_id = await _current_user(client)

    worker = _worker(db, tmp_path, ScriptedLLM([final_response("done")]))
    claimed = await claim_as(db, worker)
    assert claimed.id == task_id
    await worker.process(claimed)

    async with db() as session:
        settled = await session.get(Task, task_id)
        assert settled is not None and settled.status is TaskStatus.SUCCEEDED
        row = (
            await session.execute(sa.select(LlmUsageLog).where(LlmUsageLog.user_id == user_id))
        ).scalar_one()
        balance = await credit_balance(session, user_id)
    assert row.model_slug == MODEL
    assert row.credits_deducted > 0
    # A brand-new local-dev account starts with the default starting grant.
    assert balance == Decimal("1000") - row.credits_deducted
