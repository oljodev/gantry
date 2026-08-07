"""Paddle webhook signature verification and event->grant parsing, in
isolation — no app, no database. The real Paddle HMAC scheme (``ts=...;h1=...``
over the raw body) is reproduced here independently of the implementation, so
these tests fail if the implementation drifts from the documented wire format,
not just from itself.
"""

from __future__ import annotations

import hashlib
import hmac
from decimal import Decimal

import pytest

from gantry.billing.paddle import (
    GRANTING_EVENT_TYPES,
    SignatureVerificationError,
    extract_grant_intent,
    verify_signature,
)
from gantry.config import Settings

SECRET = "pdl_ntfset_test_secret"


def sign(body: bytes, secret: str = SECRET, ts: int = 1_700_000_000) -> str:
    """Build a valid ``Paddle-Signature`` header the way Paddle itself would."""
    digest = hmac.new(secret.encode(), f"{ts}:".encode() + body, hashlib.sha256).hexdigest()
    return f"ts={ts};h1={digest}"


# --- signature verification -----------------------------------------------


def test_a_correctly_signed_body_verifies() -> None:
    body = b'{"event_id": "evt_1"}'
    verify_signature(body, sign(body), SECRET, now=1_700_000_010)


def test_a_tampered_body_is_rejected() -> None:
    body = b'{"event_id": "evt_1"}'
    header = sign(body)
    with pytest.raises(SignatureVerificationError):
        verify_signature(b'{"event_id": "evt_EVIL"}', header, SECRET, now=1_700_000_010)


def test_the_wrong_secret_is_rejected() -> None:
    body = b'{"event_id": "evt_1"}'
    header = sign(body, secret="not-the-real-secret")
    with pytest.raises(SignatureVerificationError):
        verify_signature(body, header, SECRET, now=1_700_000_010)


def test_a_missing_header_is_rejected() -> None:
    with pytest.raises(SignatureVerificationError, match="missing"):
        verify_signature(b"{}", None, SECRET)


def test_a_malformed_header_is_rejected() -> None:
    for bad in ("", "not-the-right-shape", "ts=abc;h1=", "h1=onlyhash"):
        with pytest.raises(SignatureVerificationError):
            verify_signature(b"{}", bad, SECRET)


def test_a_non_numeric_timestamp_is_rejected() -> None:
    with pytest.raises(SignatureVerificationError):
        verify_signature(b"{}", "ts=not-a-number;h1=abc", SECRET)


def test_a_stale_timestamp_is_rejected_even_with_a_correct_hash() -> None:
    """Proves the freshness check is real, not just present: a signature that
    is cryptographically valid for its own (old) timestamp must still be
    refused — otherwise a captured request could be replayed indefinitely."""
    body = b'{"event_id": "evt_1"}'
    header = sign(body, ts=1_700_000_000)
    with pytest.raises(SignatureVerificationError, match="stale"):
        verify_signature(body, header, SECRET, tolerance_seconds=300, now=1_700_001_000)


def test_a_timestamp_within_tolerance_is_accepted() -> None:
    body = b'{"event_id": "evt_1"}'
    header = sign(body, ts=1_700_000_000)
    verify_signature(body, header, SECRET, tolerance_seconds=300, now=1_700_000_250)


def test_a_future_timestamp_beyond_tolerance_is_also_rejected() -> None:
    """Clock skew is symmetric — a timestamp implausibly far in the FUTURE is
    just as suspicious as one far in the past."""
    body = b'{"event_id": "evt_1"}'
    header = sign(body, ts=1_700_001_000)
    with pytest.raises(SignatureVerificationError):
        verify_signature(body, header, SECRET, tolerance_seconds=300, now=1_700_000_000)


# --- event -> grant intent ---------------------------------------------


def _settings(**overrides: object) -> Settings:
    return Settings(_env_file=None, **overrides)  # type: ignore[arg-type]


def _event(event_type: str, data: dict[str, object], event_id: str = "evt_1") -> dict[str, object]:
    return {"event_id": event_id, "event_type": event_type, "data": data}


def test_granting_event_types_are_exactly_the_documented_two() -> None:
    assert {"transaction.completed", "subscription.created"} == GRANTING_EVENT_TYPES


def test_a_price_id_in_the_configured_map_grants_its_flat_amount() -> None:
    event = _event(
        "transaction.completed",
        {
            "custom_data": {"user_id": "11111111-1111-1111-1111-111111111111"},
            "items": [{"price": {"id": "pri_pack_1000"}}],
        },
    )
    settings = _settings(paddle_price_credits={"pri_pack_1000": 1000.0})
    intent = extract_grant_intent(event, settings)
    assert intent is not None
    assert intent.credits == Decimal("1000")
    assert intent.price_id == "pri_pack_1000"
    assert intent.user_id == "11111111-1111-1111-1111-111111111111"


def test_an_unmapped_price_falls_back_to_the_amount_paid_in_usd() -> None:
    """No price-map entry (the realistic near-term state, before real Paddle
    products exist) still grants something sane: amount paid x credits_per_usd."""
    event = _event(
        "transaction.completed",
        {
            "custom_data": {"user_id": "11111111-1111-1111-1111-111111111111"},
            "items": [{"price": {"id": "pri_unknown"}}],
            "details": {"totals": {"grand_total": "999", "currency_code": "USD"}},
        },
    )
    settings = _settings(credits_per_usd=100.0)
    intent = extract_grant_intent(event, settings)
    assert intent is not None
    # $9.99 paid x 100 GC/USD = 999 GC.
    assert intent.credits == Decimal("999.000000")


def test_a_non_usd_total_with_no_price_mapping_grants_nothing() -> None:
    """Converting a non-USD total at a USD rate would silently over- or
    under-grant, so this must yield nothing rather than a wrong number."""
    event = _event(
        "transaction.completed",
        {
            "custom_data": {"user_id": "11111111-1111-1111-1111-111111111111"},
            "details": {"totals": {"grand_total": "999", "currency_code": "EUR"}},
        },
    )
    assert extract_grant_intent(event, _settings()) is None


def test_subscription_created_is_also_granting() -> None:
    event = _event(
        "subscription.created",
        {
            "custom_data": {"user_id": "11111111-1111-1111-1111-111111111111"},
            "items": [{"price": {"id": "pri_sub"}}],
        },
    )
    settings = _settings(paddle_price_credits={"pri_sub": 500.0})
    intent = extract_grant_intent(event, settings)
    assert intent is not None
    assert intent.credits == Decimal("500")


@pytest.mark.parametrize(
    "event_type",
    ["subscription.updated", "transaction.created", "customer.updated", "invoice.paid", ""],
)
def test_a_non_granting_event_type_yields_nothing(event_type: str) -> None:
    event = _event(event_type, {"custom_data": {"user_id": "x"}})
    assert extract_grant_intent(event, _settings()) is None


def test_a_missing_event_id_yields_nothing() -> None:
    event = {"event_type": "transaction.completed", "data": {}}
    assert extract_grant_intent(event, _settings()) is None


def test_missing_data_yields_nothing() -> None:
    event = {"event_id": "evt_1", "event_type": "transaction.completed"}
    assert extract_grant_intent(event, _settings()) is None


def test_missing_custom_data_user_id_yields_nothing() -> None:
    """A purchase made directly on paddle.com, bypassing our checkout call,
    carries no custom_data — there is no account to credit."""
    event = _event(
        "transaction.completed",
        {"items": [{"price": {"id": "pri_pack_1000"}}]},
    )
    settings = _settings(paddle_price_credits={"pri_pack_1000": 1000.0})
    assert extract_grant_intent(event, settings) is None


def test_a_non_string_user_id_yields_nothing() -> None:
    event = _event("transaction.completed", {"custom_data": {"user_id": 12345}})
    assert extract_grant_intent(event, _settings()) is None


def test_zero_creditable_amount_yields_nothing() -> None:
    event = _event(
        "transaction.completed",
        {"custom_data": {"user_id": "11111111-1111-1111-1111-111111111111"}},
    )
    assert extract_grant_intent(event, _settings()) is None


def test_malformed_data_shapes_do_not_raise() -> None:
    """A webhook shaped differently than expected must degrade to 'nothing to
    grant', never a 500 — a parsing surprise should never crash-loop Paddle's
    retries."""
    weird_events = [
        _event("transaction.completed", {"custom_data": "not-a-dict"}),
        _event("transaction.completed", {"custom_data": {"user_id": "x"}, "items": "not-a-list"}),
        _event(
            "transaction.completed",
            {"custom_data": {"user_id": "x"}, "items": [{"price": "not-a-dict"}]},
        ),
        _event(
            "transaction.completed",
            {"custom_data": {"user_id": "x"}, "details": "not-a-dict"},
        ),
        _event(
            "transaction.completed",
            {
                "custom_data": {"user_id": "x"},
                "details": {"totals": {"grand_total": "not-a-number"}},
            },
        ),
    ]
    for event in weird_events:
        assert extract_grant_intent(event, _settings()) is None
