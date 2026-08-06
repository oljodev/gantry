"""Usage metering and Gantry Credits (GC).

Layering, outermost first:

- ``metering`` — an ``LLMClient`` decorator; the single hook every call passes.
- ``ledger``   — the audit row + the atomic balance decrement, in one transaction.
- ``credits``  — pure margin math: raw provider cost -> credits charged.
- ``catalog``  — model list prices (live OpenRouter snapshot over a static table).
- ``users``    — resolving the account a run bills to.
"""

from __future__ import annotations

from gantry.billing.credits import CallCharge, calculate_credits_for_call, gross_margin
from gantry.billing.ledger import BillingContext, has_credit, record_call, run_credits_used
from gantry.billing.metering import MeteredLLMClient, meter
from gantry.billing.users import resolve_user_id

__all__ = [
    "BillingContext",
    "CallCharge",
    "MeteredLLMClient",
    "calculate_credits_for_call",
    "gross_margin",
    "has_credit",
    "meter",
    "record_call",
    "resolve_user_id",
    "run_credits_used",
]
