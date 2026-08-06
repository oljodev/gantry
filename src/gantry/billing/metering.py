"""The billing hook: an ``LLMClient`` that charges for what it returns.

``MeteredLLMClient`` wraps any client satisfying the ``LLMClient`` protocol and,
immediately after a response comes back, writes the usage row and deducts the
credits. Wrapping the *client* rather than patching a provider callback is what
makes the coverage total: every LLM call in Gantry — agent steps, the compaction
summarizer, the merge-conflict resolver, the attachment vision pre-pass — goes
through this one interface, so none of them can spend money off-ledger. A future
call site gets metered by construction rather than by remembering to instrument it.

Two properties matter more than they look:

- **The charge is priced from the REQUESTED slug**, not the provider-echoed
  ``response.model``. Providers rewrite the field (routing, version pinning,
  ``-latest`` resolution), and the model we priced must be the model we audit.
- **A billing failure never fails the agent.** By the time we run, the provider
  has already answered and already charged us; raising here would discard
  completed work over a bookkeeping error. It is logged at ERROR instead, which
  is the loudest thing that does not destroy the run.
"""

from __future__ import annotations

from collections.abc import Sequence

from gantry.billing.ledger import BillingContext, RecordedCall, Sessions, record_call_in_session
from gantry.logging import get_logger
from gantry.runtime.llm import DeltaSink, LLMClient, LLMResponse, Message, ToolSchema

logger = get_logger(__name__)


class MeteredLLMClient:
    """Decorates an ``LLMClient``, billing each completion to ``context``."""

    def __init__(
        self,
        inner: LLMClient,
        sessions: Sessions,
        context: BillingContext,
    ) -> None:
        self._inner = inner
        self._sessions = sessions
        self._context = context

    @property
    def context(self) -> BillingContext:
        return self._context

    async def complete(
        self,
        *,
        model: str,
        messages: list[Message],
        tools: Sequence[ToolSchema] = (),
        on_delta: DeltaSink | None = None,
    ) -> LLMResponse:
        response = await self._inner.complete(
            model=model, messages=messages, tools=tools, on_delta=on_delta
        )
        await self._bill(model, response)
        return response

    async def _bill(self, model: str, response: LLMResponse) -> RecordedCall | None:
        usage = response.usage
        # A provider that reported nothing (a cached/empty stream) is not a
        # billable event — writing a zero row per no-op would bloat the audit
        # trail with rows that can never explain a balance change.
        if usage.prompt_tokens == 0 and usage.completion_tokens == 0:
            return None
        try:
            recorded = await record_call_in_session(
                self._sessions, self._context, model_slug=model, usage=usage
            )
        except Exception as exc:
            logger.error(
                "billing.record_failed",
                task_id=str(self._context.task_id),
                model=model,
                prompt_tokens=usage.prompt_tokens,
                completion_tokens=usage.completion_tokens,
                error=repr(exc),
            )
            return None
        logger.info(
            "billing.call_charged",
            task_id=str(self._context.task_id),
            model=model,
            total_tokens=recorded.charge.total_tokens,
            raw_cost_usd=str(recorded.charge.raw_cost_usd),
            credits=str(recorded.charge.credits_deducted),
            balance=None if recorded.balance_after is None else str(recorded.balance_after),
        )
        return recorded


def meter(
    inner: LLMClient,
    sessions: Sessions | None,
    context: BillingContext | None,
) -> LLMClient:
    """Wrap ``inner`` for billing when there is somewhere to bill it.

    Returns the client untouched when metering is not configured (tests that
    inject a fake client and never set up a database), so the billing layer is
    strictly additive to every existing call path.
    """
    if sessions is None or context is None:
        return inner
    return MeteredLLMClient(inner, sessions, context)
