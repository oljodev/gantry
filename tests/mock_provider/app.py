"""The FastAPI app: an OpenAI-compatible ``/v1/chat/completions`` endpoint.

Emits ``chat.completion.chunk`` SSE frames that LiteLLM reassembles into a normal
response (content, tool calls, and usage). ``stream_options.include_usage`` — which
Gantry's ``LiteLLMClient`` always sets — turns on the periodic + trailing ``usage``
frames.
"""

from __future__ import annotations

import json
import time
import uuid
from collections.abc import AsyncIterator
from typing import Any

from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse, StreamingResponse
from starlette.responses import Response

from .scenarios import Turn, select, step_of


def _sse(payload: dict[str, Any]) -> str:
    return f"data: {json.dumps(payload)}\n\n"


def _usage(turn: Turn, completion_tokens: int) -> dict[str, int]:
    return {
        "prompt_tokens": turn.prompt_tokens,
        "completion_tokens": completion_tokens,
        "total_tokens": turn.prompt_tokens + completion_tokens,
    }


async def _stream_turn(model: str, turn: Turn, include_usage: bool) -> AsyncIterator[str]:
    """Yield the SSE frames for one scripted turn, OpenAI-chunk shaped."""
    cid = f"chatcmpl-mock-{uuid.uuid4().hex[:12]}"
    created = int(time.time())

    def frame(choices: list[dict[str, Any]], usage: dict[str, int] | None = None) -> str:
        body: dict[str, Any] = {
            "id": cid,
            "object": "chat.completion.chunk",
            "created": created,
            "model": model,
            "choices": choices,
        }
        if usage is not None:
            body["usage"] = usage
        return _sse(body)

    def delta(d: dict[str, Any], finish: str | None = None) -> str:
        return frame([{"index": 0, "delta": d, "finish_reason": finish}])

    # 1. Role preamble.
    yield delta({"role": "assistant"})

    # 2. Content wall, streamed across content_chunks with periodic usage frames.
    if turn.content:
        chunks = max(1, turn.content_chunks)
        per_chunk = max(1, turn.completion_tokens // chunks)
        streamed = 0
        for i in range(chunks):
            # A multi-chunk wall varies each frame: identical consecutive chunks
            # trip LiteLLM's "model is repeating itself" guard. Single-chunk
            # content is emitted verbatim.
            piece = turn.content if chunks == 1 else f"{turn.content}{i} "
            yield delta({"content": piece})
            streamed += per_chunk
            if include_usage and turn.usage_every and (i + 1) % turn.usage_every == 0:
                yield frame([], _usage(turn, streamed))

    # 3. Tool calls.
    for idx, tc in enumerate(turn.tool_calls):
        yield delta(
            {
                "tool_calls": [
                    {
                        "index": idx,
                        "id": tc.id,
                        "type": "function",
                        "function": {"name": tc.name, "arguments": json.dumps(tc.arguments)},
                    }
                ]
            }
        )

    # 4. Terminal finish_reason frame.
    yield delta({}, turn.finish_reason)

    # 5. Trailing usage frame (OpenAI's include_usage contract: a final choice-less
    #    chunk carrying the total).
    if include_usage:
        yield frame([], _usage(turn, turn.completion_tokens))

    yield "data: [DONE]\n\n"


def _completion_body(model: str, turn: Turn) -> dict[str, Any]:
    """A non-streaming ``chat.completion`` body (for clients that don't stream)."""
    message: dict[str, Any] = {"role": "assistant", "content": turn.content or None}
    if turn.tool_calls:
        message["tool_calls"] = [
            {
                "id": tc.id,
                "type": "function",
                "function": {"name": tc.name, "arguments": json.dumps(tc.arguments)},
            }
            for tc in turn.tool_calls
        ]
    return {
        "id": f"chatcmpl-mock-{uuid.uuid4().hex[:12]}",
        "object": "chat.completion",
        "created": int(time.time()),
        "model": model,
        "choices": [{"index": 0, "message": message, "finish_reason": turn.finish_reason}],
        "usage": _usage(turn, turn.completion_tokens),
    }


def create_app() -> FastAPI:
    app = FastAPI(title="Gantry Mock Provider")

    @app.get("/health")
    async def health() -> dict[str, str]:
        return {"status": "ok"}

    @app.post("/v1/chat/completions")
    async def chat_completions(request: Request) -> Response:
        body = await request.json()
        model = str(body.get("model") or "mock/happy-path")
        messages = list(body.get("messages") or [])
        include_usage = bool((body.get("stream_options") or {}).get("include_usage"))
        turn = select(model)(step_of(messages), messages)
        if body.get("stream"):
            return StreamingResponse(
                _stream_turn(model, turn, include_usage), media_type="text/event-stream"
            )
        return JSONResponse(_completion_body(model, turn))

    return app
