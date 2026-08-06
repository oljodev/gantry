"""The runtime credit gate: when to stop an agent, without a query per step.

An agent checks "can I still afford this?" at every step boundary. Doing that as
a database read would add a round-trip to every step of every agent in the fleet
— for a 100-agent swarm at 150 steps each, 15,000 queries that almost always say
yes.

They are unnecessary because the metering path already knows the answer: each
deduction returns the post-charge balance (``UPDATE ... RETURNING``), which is
the freshest possible reading and costs nothing extra. So the gate is a small
mutable object that the metering client updates after every call and the loop
reads synchronously. One database read seeds it when the task is claimed.

The gate is intentionally conservative in one direction only: it trips on
``balance <= 0`` observed *after* a charge. It cannot predict what the next call
will cost — that is unknowable before the call is made — so an account always
overruns its last credit by at most one call. Bounded, visible, and far better
than refusing to start work we cannot price yet.
"""

from __future__ import annotations

from decimal import Decimal


class CreditGate:
    """Tracks an account's balance as the metering path reports it.

    Shared by one task's LLM client and its step callback. Not thread-safe and
    does not need to be: a task's steps are sequential on one event loop.
    """

    def __init__(self, *, enabled: bool, balance: Decimal | None = None) -> None:
        #: When enforcement is off the gate answers "funded" no matter what, so
        #: the observation path stays live (and testable) without gating anything.
        self._enabled = enabled
        self._balance = balance

    @property
    def enabled(self) -> bool:
        return self._enabled

    @property
    def balance(self) -> Decimal | None:
        """Last observed balance. None means unknown or unowned — never gated."""
        return self._balance

    def observe(self, balance: Decimal | None) -> None:
        """Record the balance a deduction just returned.

        A None observation (an unowned call, or a user row that has since been
        deleted) is ignored rather than treated as zero: "we don't know" must not
        read as "out of money" and pause a run that nobody is billing.
        """
        if balance is not None:
            self._balance = balance

    @property
    def exhausted(self) -> bool:
        """Whether the agent should pause before starting more work."""
        return self._enabled and self._balance is not None and self._balance <= 0

    def describe(self) -> str:
        """The balance as a string for the pause event — the number the user is
        shown, recorded at the moment the decision was made."""
        return "" if self._balance is None else f"{self._balance:.6f}"
