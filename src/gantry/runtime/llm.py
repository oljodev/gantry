"""Provider-agnostic LLM abstraction.

The loop talks to this interface only. ``LiteLLMClient`` adapts any provider
LiteLLM supports (model strings like ``anthropic/claude-opus-4-8`` or
``openai/gpt-5``); tests inject deterministic fakes. Messages use the
OpenAI-style dict format LiteLLM speaks natively.
"""

from __future__ import annotations

import json
from collections.abc import Sequence
from dataclasses import dataclass, field
from typing import Any, Protocol

#: OpenAI-style chat message dict: {"role": ..., "content": ..., ...}
Message = dict[str, Any]
#: OpenAI-style function tool schema dict.
ToolSchema = dict[str, Any]


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


class LLMClient(Protocol):
    async def complete(
        self,
        *,
        model: str,
        messages: list[Message],
        tools: Sequence[ToolSchema] = (),
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
    ) -> None:
        self._api_key = api_key
        self._api_base = api_base
        self._prompt_caching = prompt_caching

    async def complete(
        self,
        *,
        model: str,
        messages: list[Message],
        tools: Sequence[ToolSchema] = (),
    ) -> LLMResponse:
        import litellm

        payload = with_cache_control(messages, model) if self._prompt_caching else messages
        kwargs: dict[str, Any] = {"model": model, "messages": payload}
        if tools:
            kwargs["tools"] = list(tools)
        if self._api_key:
            kwargs["api_key"] = self._api_key
        if self._api_base:
            kwargs["api_base"] = self._api_base
        raw: Any = await litellm.acompletion(**kwargs)

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
        return LLMResponse(
            content=message.content,
            tool_calls=tool_calls,
            model=str(raw.model or model),
            usage=_usage_from(raw.usage),
            finish_reason=str(choice.finish_reason or "stop"),
        )
