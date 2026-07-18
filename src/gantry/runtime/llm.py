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
    """

    async def complete(
        self,
        *,
        model: str,
        messages: list[Message],
        tools: Sequence[ToolSchema] = (),
    ) -> LLMResponse:
        import litellm

        kwargs: dict[str, Any] = {"model": model, "messages": messages}
        if tools:
            kwargs["tools"] = list(tools)
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
        usage = LLMUsage(
            prompt_tokens=int(getattr(raw.usage, "prompt_tokens", 0) or 0),
            completion_tokens=int(getattr(raw.usage, "completion_tokens", 0) or 0),
        )
        return LLMResponse(
            content=message.content,
            tool_calls=tool_calls,
            model=str(raw.model or model),
            usage=usage,
            finish_reason=str(choice.finish_reason or "stop"),
        )
