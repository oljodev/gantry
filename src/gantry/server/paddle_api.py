"""``POST /api/webhooks/paddle`` — the inbound side of the payment integration.

Deliberately outside every other router's auth machinery: the caller is
Paddle's servers, which cannot present a Supabase bearer token or our internal
admin secret. Its own HMAC signature (see :mod:`gantry.billing.paddle`) IS the
authentication, verified against the raw request body — so this handler must
read ``request.body()`` before anything else touches the request, and must
never call ``request.json()`` first (that would let a body Starlette re-encodes
differently sail past a signature computed over the original bytes).
"""

from __future__ import annotations

import json
import uuid
from typing import cast

from fastapi import APIRouter, HTTPException, Request
from sqlalchemy.dialects.postgresql import insert as pg_insert
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.billing.ledger import grant_credits_and_resume
from gantry.billing.paddle import SignatureVerificationError, extract_grant_intent, verify_signature
from gantry.config import Settings
from gantry.core.db import session_scope
from gantry.core.models import PaddleWebhookEvent
from gantry.logging import get_logger
from gantry.server.schemas import PaddleWebhookResponse

logger = get_logger(__name__)

router = APIRouter(prefix="/api/webhooks", tags=["webhooks"])

Sessions = async_sessionmaker[AsyncSession]

#: Paddle's own header name for the HMAC signature — not configurable, it's
#: their wire format.
SIGNATURE_HEADER = "Paddle-Signature"


@router.post("/paddle", response_model=PaddleWebhookResponse)
async def paddle_webhook(request: Request) -> PaddleWebhookResponse:
    settings = cast("Settings", request.app.state.settings)
    raw_body = await request.body()

    # An unconfigured secret must fail CLOSED — never fall through to "accept
    # unsigned", which is what happens if this check is skipped when the env
    # var simply hasn't been set yet (exactly the state this integration ships
    # in today, ahead of the real Paddle account).
    if not settings.paddle_webhook_secret:
        logger.error("paddle.webhook_secret_not_configured")
        raise HTTPException(status_code=401, detail="Paddle webhook is not configured")

    try:
        verify_signature(
            raw_body,
            request.headers.get(SIGNATURE_HEADER),
            settings.paddle_webhook_secret,
            tolerance_seconds=settings.paddle_signature_tolerance_seconds,
        )
    except SignatureVerificationError as exc:
        logger.warning("paddle.signature_rejected", error=str(exc))
        raise HTTPException(status_code=401, detail="invalid signature") from exc

    try:
        event = json.loads(raw_body)
    except ValueError as exc:
        raise HTTPException(status_code=400, detail="malformed JSON body") from exc
    if not isinstance(event, dict):
        raise HTTPException(status_code=400, detail="expected a JSON object")

    intent = extract_grant_intent(event, settings)
    if intent is None:
        # Verified, parsed, and deliberately not actionable (wrong event type,
        # or one we couldn't map to a fundable account/amount) — see
        # extract_grant_intent for why this is "skip", not an error.
        return PaddleWebhookResponse(status="skipped", event_id=event.get("event_id"))

    try:
        user_id = uuid.UUID(intent.user_id)
    except ValueError:
        logger.warning(
            "paddle.custom_data_user_id_not_a_uuid",
            event_id=intent.event_id,
            raw=intent.user_id,
        )
        return PaddleWebhookResponse(status="skipped", event_id=intent.event_id)

    sessions = cast("Sessions", request.app.state.sessions)
    async with session_scope(sessions) as session:
        # The idempotency marker and the grant land in ONE transaction: if a
        # retry's marker insert collides, we return before touching the
        # balance; if it doesn't collide, the grant that follows either commits
        # together with the marker or (on any error) rolls back together with
        # it, so a failed delivery is never half-recorded as "seen".
        inserted = (
            await session.execute(
                pg_insert(PaddleWebhookEvent)
                .values(
                    event_id=intent.event_id,
                    event_type=intent.event_type,
                    user_id=user_id,
                    credits_granted=intent.credits,
                )
                .on_conflict_do_nothing(index_elements=[PaddleWebhookEvent.event_id])
                .returning(PaddleWebhookEvent.event_id)
            )
        ).first()
        if inserted is None:
            logger.info("paddle.duplicate_event", event_id=intent.event_id)
            return PaddleWebhookResponse(status="duplicate", event_id=intent.event_id)

        outcome = await grant_credits_and_resume(session, user_id, intent.credits)
        if outcome is None:
            # A well-formed, signed event for an account that does not exist in
            # this workspace — most likely custom_data carrying a stale or
            # foreign id. The marker row still commits, so a Paddle retry of the
            # SAME event doesn't re-alert on every redelivery; a genuinely new
            # event for the same (bad) id will, which is what we want.
            logger.error(
                "paddle.grant_target_not_found", event_id=intent.event_id, user_id=str(user_id)
            )
            return PaddleWebhookResponse(status="skipped", event_id=intent.event_id)

    logger.info(
        "paddle.credits_granted",
        event_id=intent.event_id,
        event_type=intent.event_type,
        user_id=str(user_id),
        credits=str(intent.credits),
        resumed_tasks=outcome.resumed_tasks,
        price_id=intent.price_id,
    )
    return PaddleWebhookResponse(
        status="granted",
        event_id=intent.event_id,
        user_id=user_id,
        credits_granted=float(intent.credits),
        resumed_tasks=outcome.resumed_tasks,
    )
