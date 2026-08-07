"""Paddle (Merchant of Record) webhook: signature verification and event
parsing, both pure functions over bytes/dicts so they are testable without a
live Paddle account — which does not exist yet (see ``config.paddle_*``).

Paddle Billing (the current API, as opposed to the legacy "Paddle Classic" RSA
scheme) signs every webhook with HMAC-SHA256 over the raw request body, using
the secret shown for that notification destination in the Paddle dashboard. The
signature travels in a ``Paddle-Signature`` header shaped like
``ts=1671552777;h1=<hex digest>``. There is no public key involved — that is a
detail from the deprecated Classic API, and building RSA verification here
would be implementing a scheme Paddle no longer uses.

Everything downstream of "is this signature valid" is deliberately written
against the payload's actual shape rather than an SDK model, because the
integration is being built ahead of the Paddle account it will run against.
The identifying fields it depends on (``event_id``, ``event_type``,
``data.custom_data.user_id``, ``data.items[].price.id``) are stable across
Paddle Billing's ``transaction.*`` and ``subscription.*`` events; unexpected or
missing shape degrades to "nothing to grant" rather than raising, so a webhook
that doesn't parse the way we guessed 200s instead of crash-looping Paddle's
retries.
"""

from __future__ import annotations

import hashlib
import hmac
import time
from dataclasses import dataclass
from decimal import Decimal, InvalidOperation
from typing import Any

from gantry.config import Settings
from gantry.logging import get_logger

logger = get_logger(__name__)

#: Event types that grant credits. Every other Paddle event (subscription
#: renewals, refunds, customer updates, ...) is acknowledged and ignored —
#: acting on events outside this set is future scope, not a bug.
GRANTING_EVENT_TYPES = frozenset({"transaction.completed", "subscription.created"})


class SignatureVerificationError(Exception):
    """The webhook's signature header was missing, malformed, or did not match."""


def _parse_signature_header(header: str) -> tuple[int, str] | None:
    """``"ts=1671552777;h1=abc123"`` -> ``(1671552777, "abc123")``, or None."""
    parts: dict[str, str] = {}
    for segment in header.split(";"):
        key, _, value = segment.partition("=")
        if key and value:
            parts[key.strip()] = value.strip()
    ts, h1 = parts.get("ts"), parts.get("h1")
    if ts is None or not h1:
        return None
    try:
        return int(ts), h1
    except ValueError:
        return None


def verify_signature(
    raw_body: bytes,
    header: str | None,
    secret: str,
    *,
    tolerance_seconds: float = 300.0,
    now: float | None = None,
) -> None:
    """Raise :class:`SignatureVerificationError` unless ``header`` proves this
    body was signed by ``secret`` and is recent.

    Two checks, both required: the HMAC must match (proves authenticity — only
    someone holding the dashboard secret could have produced it) AND the signed
    timestamp must be within ``tolerance_seconds`` of now (proves freshness — a
    valid signature captured off the wire once cannot be replayed indefinitely).
    Comparison is via ``hmac.compare_digest`` so a near-miss guess leaks nothing
    through response-time timing.

    ``now`` is a parameter (not read internally as the default) purely for
    deterministic tests — production callers never pass it.
    """
    if not header:
        raise SignatureVerificationError("missing Paddle-Signature header")
    parsed = _parse_signature_header(header)
    if parsed is None:
        raise SignatureVerificationError("malformed Paddle-Signature header")
    ts, presented_h1 = parsed

    signed_payload = f"{ts}:".encode() + raw_body
    expected_h1 = hmac.new(secret.encode(), signed_payload, hashlib.sha256).hexdigest()
    if not hmac.compare_digest(presented_h1, expected_h1):
        raise SignatureVerificationError("signature does not match")

    age = (now if now is not None else time.time()) - ts
    if abs(age) > tolerance_seconds:
        raise SignatureVerificationError(f"signed timestamp is {age:.0f}s old — rejected as stale")


@dataclass(frozen=True)
class GrantIntent:
    """What one webhook event asks us to do, once we've decided it asks for
    anything at all. Pure data — the handler is the only thing that touches the
    database."""

    event_id: str
    event_type: str
    user_id: str
    credits: Decimal
    #: For the idempotency row / support debugging — not otherwise consumed.
    price_id: str | None = None


def _to_decimal(raw: Any) -> Decimal | None:
    if raw is None:
        return None
    try:
        return Decimal(str(raw))
    except InvalidOperation:
        return None


def _price_ids(data: dict[str, Any]) -> list[str]:
    items = data.get("items")
    if not isinstance(items, list):
        return []
    ids = []
    for item in items:
        if isinstance(item, dict):
            price = item.get("price")
            if isinstance(price, dict) and isinstance(price.get("id"), str):
                ids.append(price["id"])
    return ids


def _credits_from_price_map(data: dict[str, Any], price_credits: dict[str, float]) -> Decimal:
    """Sum of the configured credit grant for every priced line item recognised
    in ``price_credits``. Zero (not None) when nothing matched, so the caller
    can fall through to the amount-based estimate uniformly."""
    total = Decimal(0)
    for price_id in _price_ids(data):
        amount = price_credits.get(price_id)
        if amount is not None:
            total += Decimal(str(amount))
    return total


def _credits_from_amount_paid(data: dict[str, Any], credits_per_usd: float) -> Decimal:
    """Fallback when no line item matches the configured price map: convert
    whatever Paddle reports as the grand total into credits at the same
    USD-per-credit rate the rest of the ledger uses.

    Only trusted for USD — Paddle settles in the seller's payout currency but
    reports totals in the transaction's own currency, and converting a non-USD
    total at a USD rate would silently over- or under-grant. A non-USD total
    with no price-map entry yields zero credits (nothing granted, loudly logged)
    rather than a wrong number quietly granted.
    """
    details = data.get("details")
    totals = details.get("totals") if isinstance(details, dict) else None
    if not isinstance(totals, dict):
        return Decimal(0)
    currency = str(totals.get("currency_code") or data.get("currency_code") or "").upper()
    if currency and currency != "USD":
        logger.warning("paddle.non_usd_total_ignored", currency=currency)
        return Decimal(0)
    # Paddle quotes totals in the currency's smallest unit (cents for USD).
    minor_units = _to_decimal(totals.get("grand_total"))
    if minor_units is None:
        return Decimal(0)
    return (minor_units / Decimal(100) * Decimal(str(credits_per_usd))).quantize(
        Decimal("0.000001")
    )


def extract_grant_intent(event: dict[str, Any], settings: Settings) -> GrantIntent | None:
    """Decide what (if anything) a verified, parsed webhook body asks us to do.

    Returns None — never raises — for anything that isn't actionable: an event
    type we don't grant on, a missing event/user id, or a purchase that maps to
    zero credits. The handler's job is then simply "acknowledge and do nothing",
    which is the correct response to a webhook shaped in a way we didn't
    anticipate, not an error condition.
    """
    event_type = str(event.get("event_type") or "")
    if event_type not in GRANTING_EVENT_TYPES:
        return None
    event_id = event.get("event_id")
    if not isinstance(event_id, str) or not event_id:
        logger.warning("paddle.event_missing_id", event_type=event_type)
        return None

    data = event.get("data")
    if not isinstance(data, dict):
        logger.warning("paddle.event_missing_data", event_id=event_id)
        return None

    custom_data = data.get("custom_data")
    user_id = custom_data.get("user_id") if isinstance(custom_data, dict) else None
    if not isinstance(user_id, str) or not user_id:
        # The checkout that created this transaction never attached custom_data
        # (e.g. a purchase made directly on paddle.com, bypassing our checkout
        # call) — there is no account to credit. Loud, not fatal.
        logger.warning("paddle.event_missing_user_id", event_id=event_id, event_type=event_type)
        return None

    credits = _credits_from_price_map(data, settings.paddle_price_credits)
    if credits <= 0:
        credits = _credits_from_amount_paid(data, settings.credits_per_usd)
    if credits <= 0:
        logger.warning(
            "paddle.event_no_creditable_amount", event_id=event_id, event_type=event_type
        )
        return None

    price_ids = _price_ids(data)
    return GrantIntent(
        event_id=event_id,
        event_type=event_type,
        user_id=user_id,
        credits=credits,
        price_id=price_ids[0] if price_ids else None,
    )
