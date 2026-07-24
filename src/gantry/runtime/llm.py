"""Provider-agnostic LLM abstraction.

The loop talks to this interface only. ``LiteLLMClient`` adapts any provider
LiteLLM supports (model strings like ``anthropic/claude-opus-4-8`` or
``openai/gpt-5``); tests inject deterministic fakes. Messages use the
OpenAI-style dict format LiteLLM speaks natively.
"""

from __future__ import annotations

import contextlib
import json
import logging
import re
from collections.abc import Awaitable, Callable, Sequence
from dataclasses import dataclass, field, replace
from typing import Any, Protocol

from gantry.runtime.ratelimit import AsyncRateLimiter

_litellm_quieted = False


def _quiet_litellm(litellm: Any) -> None:
    """Silence LiteLLM's per-call INFO spam ('Provider List: ...', 'LiteLLM
    completion() ...') so a failing model doesn't flood the worker log. Runs
    once. Real errors still propagate to us as exceptions."""
    global _litellm_quieted
    if _litellm_quieted:
        return
    with contextlib.suppress(Exception):
        litellm.suppress_debug_info = True
        # Providers that don't accept a param (e.g. parallel_tool_calls on some
        # OpenRouter models) drop it rather than 400. The only optional param we
        # add is parallel_tool_calls; tools/api_base/api_key are universally
        # supported, so this can't silently mask a genuinely-needed field.
        litellm.drop_params = True
    logging.getLogger("LiteLLM").setLevel(logging.WARNING)
    _litellm_quieted = True


#: OpenAI-style chat message dict: {"role": ..., "content": ..., ...}
Message = dict[str, Any]
#: OpenAI-style function tool schema dict.
ToolSchema = dict[str, Any]
#: Live-delta sink: (kind, text) where kind is "reasoning" or "content".
#: The loop passes one to stream a thinking model's tokens as they arrive.
DeltaSink = Callable[[str, str], Awaitable[None]]


@dataclass(frozen=True)
class ToolCallRequest:
    id: str
    name: str
    arguments: dict[str, Any]


@dataclass(frozen=True)
class LLMUsage:
    prompt_tokens: int = 0
    completion_tokens: int = 0
    #: Input tokens served from the provider's prompt cache (cheap re-reads).
    cache_read_tokens: int = 0
    #: Input tokens written into the prompt cache this call (one-time surcharge).
    cache_write_tokens: int = 0


def _usage_from(raw_usage: Any) -> LLMUsage:
    """Read token counts off a LiteLLM usage object, including Anthropic's
    prompt-cache fields (absent/zero for providers or calls without caching)."""
    read = int(getattr(raw_usage, "cache_read_input_tokens", 0) or 0)
    if not read:
        details = getattr(raw_usage, "prompt_tokens_details", None)
        read = int(getattr(details, "cached_tokens", 0) or 0) if details is not None else 0
    return LLMUsage(
        prompt_tokens=int(getattr(raw_usage, "prompt_tokens", 0) or 0),
        completion_tokens=int(getattr(raw_usage, "completion_tokens", 0) or 0),
        cache_read_tokens=read,
        cache_write_tokens=int(getattr(raw_usage, "cache_creation_input_tokens", 0) or 0),
    )


@dataclass(frozen=True)
class LLMResponse:
    content: str | None
    tool_calls: tuple[ToolCallRequest, ...] = ()
    model: str = ""
    usage: LLMUsage = field(default_factory=LLMUsage)
    finish_reason: str = "stop"
    #: A thinking model's internal reasoning for this turn (DeepSeek R1's
    #: reasoning_content etc.). Recorded for display; deliberately NOT replayed
    #: into later turns' history — provider specs require dropping it, and the
    #: assistant message reconstruction only ever carries content + tool_calls.
    reasoning: str = ""


class LLMClient(Protocol):
    async def complete(
        self,
        *,
        model: str,
        messages: list[Message],
        tools: Sequence[ToolSchema] = (),
        on_delta: DeltaSink | None = None,
    ) -> LLMResponse: ...


def _model_supports_caching(model: str) -> bool:
    """Anthropic (incl. Bedrock/Vertex Claude) is what LiteLLM lets us set
    ``cache_control`` on. Everything else must be left untouched — an unknown
    key can trip other providers' request validation."""
    m = model.lower()
    return "claude" in m or "anthropic" in m


def _mark_cache(message: Message) -> Message:
    """Return a shallow copy of ``message`` with an ephemeral cache breakpoint
    on its final text block, promoting a plain string body to block form. A
    message with no cacheable text (e.g. an assistant turn that is only
    tool_calls) is returned unchanged."""
    content = message.get("content")
    if isinstance(content, str):
        if not content:
            return message
        blocks: list[dict[str, Any]] = [{"type": "text", "text": content}]
    elif isinstance(content, list) and content:
        blocks = [dict(b) if isinstance(b, dict) else b for b in content]
    else:
        return message
    last = blocks[-1]
    if not isinstance(last, dict):
        return message
    blocks[-1] = {**last, "cache_control": {"type": "ephemeral"}}
    return {**message, "content": blocks}


def with_cache_control(messages: list[Message], model: str) -> list[Message]:
    """Add Anthropic prompt-cache breakpoints so each loop step re-reads the
    prompt at cache rates instead of paying full input price.

    Two breakpoints (Anthropic allows four):

    - the **system** message — the static prefix (framework instructions,
      injected skills). Because Anthropic caches ``tools → system → messages``
      in order, this breakpoint also covers the tool schemas that precede it.
    - the **last** message — the rolling prefix. Marking the tail each step
      writes a cache of the whole conversation so far; the next step reads it
      as a hit and extends it, so the append-only history stays cached.

    A no-op for providers LiteLLM can't pass ``cache_control`` to, and for the
    degenerate empty/single-message cases.
    """
    if not messages or not _model_supports_caching(model):
        return messages
    out = list(messages)
    if out[0].get("role") == "system":
        out[0] = _mark_cache(out[0])
    if len(out) > 1:
        out[-1] = _mark_cache(out[-1])
    return out


def _request_kwargs(
    model: str,
    messages: list[Message],
    tools: Sequence[ToolSchema],
    api_key: str | None,
    api_base: str | None,
) -> dict[str, Any]:
    """Build the litellm.acompletion kwargs (pure — testable without litellm)."""
    kwargs: dict[str, Any] = {"model": model, "messages": messages}
    if tools:
        kwargs["tools"] = list(tools)
        # Let a model emit several tool calls in one turn (e.g. a leader firing a
        # whole spawn batch, or a worker batching reads). Providers that don't
        # support it drop it via litellm.drop_params.
        kwargs["parallel_tool_calls"] = True
    if api_key:
        kwargs["api_key"] = api_key
    if api_base:
        kwargs["api_base"] = api_base
    return kwargs


def parse_tool_arguments(raw: str | None) -> dict[str, Any]:
    """Best-effort parse of a tool-call arguments JSON string.

    Malformed JSON becomes an inspectable payload instead of an exception —
    the tool (or the LLM, via the error result) deals with it.
    """
    if not raw:
        return {}
    try:
        parsed = json.loads(raw)
    except ValueError:
        return {"__malformed_json__": raw}
    if not isinstance(parsed, dict):
        return {"__non_object_arguments__": parsed}
    return parsed


class LiteLLMClient:
    """LLM client over ``litellm.acompletion``.

    litellm is imported lazily: it is a heavy import and nothing else in
    Gantry (including the whole test suite) should pay for it.

    ``api_key``/``api_base`` come from a vault-backed provider config (or are
    None to fall back to ambient env vars like ``ANTHROPIC_API_KEY``).
    """

    def __init__(
        self,
        api_key: str | None = None,
        api_base: str | None = None,
        *,
        prompt_caching: bool = True,
        limiter: AsyncRateLimiter | None = None,
    ) -> None:
        self._api_key = api_key
        self._api_base = api_base
        self._prompt_caching = prompt_caching
        #: Shared, process-wide outbound pacer (None = unthrottled, e.g. tests).
        self._limiter = limiter

    async def complete(
        self,
        *,
        model: str,
        messages: list[Message],
        tools: Sequence[ToolSchema] = (),
        on_delta: DeltaSink | None = None,
    ) -> LLMResponse:
        import litellm

        _quiet_litellm(litellm)
        # Pace the outbound stream before spending any tokens — under high
        # concurrency this queues bursts to avoid provider 429 spikes.
        if self._limiter is not None:
            await self._limiter.acquire()

        payload = with_cache_control(messages, model) if self._prompt_caching else messages
        kwargs = _request_kwargs(model, payload, tools, self._api_key, self._api_base)

        if on_delta is None:
            raw: Any = await litellm.acompletion(**kwargs)
            return _response_from_raw(raw, model)

        # Streaming path: forward reasoning/content deltas live, then rebuild the
        # full response (tool-call fragments included) from the collected chunks.
        kwargs["stream"] = True
        kwargs["stream_options"] = {"include_usage": True}
        chunks: list[Any] = []
        reasoning_parts: list[str] = []
        stream = await litellm.acompletion(**kwargs)
        async for chunk in stream:
            chunks.append(chunk)
            delta = _chunk_delta(chunk)
            if delta is None:
                continue
            reasoning = getattr(delta, "reasoning_content", None) or getattr(
                delta, "reasoning", None
            )
            if reasoning:
                reasoning_parts.append(str(reasoning))
                await on_delta("reasoning", str(reasoning))
            content = getattr(delta, "content", None)
            if content:
                await on_delta("content", str(content))

        rebuilt = litellm.stream_chunk_builder(chunks, messages=payload)
        response = _response_from_raw(rebuilt, model)
        reasoning_text = "".join(reasoning_parts)
        return replace(response, reasoning=reasoning_text) if reasoning_text else response


def _chunk_delta(chunk: Any) -> Any:
    """The delta object of a streaming chunk, or None for a usage-only chunk."""
    choices = getattr(chunk, "choices", None) or []
    return getattr(choices[0], "delta", None) if choices else None


_THINK_RE = re.compile(r"<think>(.*?)</think>", re.DOTALL | re.IGNORECASE)


def _split_reasoning(content: str | None, native: str) -> tuple[str | None, str]:
    """Separate reasoning from answer content.

    A structured ``reasoning_content`` field (DeepSeek R1) is authoritative. Some
    models instead inline their reasoning as ``<think>...</think>`` in the
    content; extract those into reasoning and strip them from the answer. A plain
    model with neither (e.g. a standard Flash turn) is returned untouched, so its
    prose never renders as a thinking block.
    """
    if not content or "<think>" not in content.lower():
        return content, native
    extracted = "\n".join(m.strip() for m in _THINK_RE.findall(content))
    cleaned = _THINK_RE.sub("", content).strip()
    combined = "\n".join(part for part in (native, extracted) if part)
    return (cleaned or None), combined


def _response_from_raw(raw: Any, model: str) -> LLMResponse:
    """Adapt a LiteLLM (streamed or not) response into our LLMResponse."""
    if raw is None:  # an empty stream — no content, no calls
        return LLMResponse(content=None, model=model)
    choice = raw.choices[0]
    message = choice.message
    tool_calls = tuple(
        ToolCallRequest(
            id=tc.id,
            name=tc.function.name,
            arguments=parse_tool_arguments(tc.function.arguments),
        )
        for tc in (message.tool_calls or [])
    )
    native = getattr(message, "reasoning_content", None) or getattr(message, "reasoning", None)
    content, reasoning = _split_reasoning(message.content, str(native) if native else "")
    return LLMResponse(
        content=content,
        tool_calls=tool_calls,
        model=str(raw.model or model),
        usage=_usage_from(raw.usage),
        finish_reason=str(choice.finish_reason or "stop"),
        reasoning=reasoning,
    )
