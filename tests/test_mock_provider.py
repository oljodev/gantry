"""Mock-level tests: drive the provider app directly (ASGI transport, no socket)
and assert its SSE shape, scenario switching, tool-call sequences, and usage."""

from __future__ import annotations

import json
from typing import Any

import httpx

from .mock_provider.app import create_app
from .mock_provider.scenarios import (
    HAPPY_WRITE_PATH,
    REPAIR_BODY,
    REPAIR_PATH,
    RUNAWAY_CHUNKS,
)

Message = dict[str, Any]


def _history(assistant_turns: int) -> list[Message]:
    """A message list standing at step ``assistant_turns`` (that many prior model
    turns), which is how the mock derives the current step."""
    messages: list[Message] = [
        {"role": "system", "content": "sys"},
        {"role": "user", "content": "go"},
    ]
    for i in range(assistant_turns):
        messages.append({"role": "assistant", "content": f"turn {i}"})
        messages.append({"role": "tool", "content": "result"})
    return messages


async def _stream(
    model: str, messages: list[Message], *, include_usage: bool = True
) -> list[dict[str, Any]]:
    body: dict[str, Any] = {"model": model, "messages": messages, "stream": True}
    if include_usage:
        body["stream_options"] = {"include_usage": True}
    transport = httpx.ASGITransport(app=create_app())
    chunks: list[dict[str, Any]] = []
    saw_done = False
    async with (
        httpx.AsyncClient(transport=transport, base_url="http://mock") as client,
        client.stream("POST", "/v1/chat/completions", json=body) as resp,
    ):
        assert resp.headers["content-type"].startswith("text/event-stream")
        async for line in resp.aiter_lines():
            if not line.startswith("data: "):
                continue
            data = line[len("data: ") :].strip()
            if data == "[DONE]":
                saw_done = True
                break
            chunks.append(json.loads(data))
    assert saw_done, "stream never terminated with [DONE]"
    return chunks


def _tool_calls(chunks: list[dict[str, Any]]) -> list[dict[str, Any]]:
    calls: list[dict[str, Any]] = []
    for chunk in chunks:
        for choice in chunk.get("choices", []):
            calls.extend(choice.get("delta", {}).get("tool_calls", []) or [])
    return calls


def _content(chunks: list[dict[str, Any]]) -> str:
    parts: list[str] = []
    for chunk in chunks:
        for choice in chunk.get("choices", []):
            piece = choice.get("delta", {}).get("content")
            if piece:
                parts.append(piece)
    return "".join(parts)


def _finish_reasons(chunks: list[dict[str, Any]]) -> list[str]:
    return [
        choice["finish_reason"]
        for chunk in chunks
        for choice in chunk.get("choices", [])
        if choice.get("finish_reason")
    ]


def _usage_chunks(chunks: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return [c for c in chunks if "usage" in c]


async def test_happy_path_walks_read_then_write_then_finish() -> None:
    step0 = _tool_calls(await _stream("mock/happy-path", _history(0)))
    assert [c["function"]["name"] for c in step0] == ["read_file"]

    step1 = _tool_calls(await _stream("mock/happy-path", _history(1)))
    assert [c["function"]["name"] for c in step1] == ["write_file"]
    assert json.loads(step1[0]["function"]["arguments"])["path"] == HAPPY_WRITE_PATH

    final = await _stream("mock/happy-path", _history(2))
    assert _tool_calls(final) == []
    assert "two steps" in _content(final).lower()
    assert _finish_reasons(final) == ["stop"]


async def test_repair_loop_returns_the_identical_broken_write_every_step() -> None:
    for step in (0, 1, 5):
        calls = _tool_calls(await _stream("mock/repair-loop", _history(step)))
        assert [c["function"]["name"] for c in calls] == ["write_file"]
        args = json.loads(calls[0]["function"]["arguments"])
        assert args == {"path": REPAIR_PATH, "content": REPAIR_BODY}


async def test_budget_runaway_streams_a_wall_with_periodic_usage() -> None:
    chunks = await _stream("mock/budget-runaway", _history(0))
    content_frames = [
        c
        for c in chunks
        for choice in c.get("choices", [])
        if choice.get("delta", {}).get("content")
    ]
    assert len(content_frames) == RUNAWAY_CHUNKS
    # Usage is streamed PERIODICALLY mid-run, not only at the end.
    assert len(_usage_chunks(chunks)) > 1
    # It never finishes: each turn ends on a tool call so the loop keeps going.
    assert [c["function"]["name"] for c in _tool_calls(chunks)] == ["list_dir"]
    assert _finish_reasons(chunks) == ["tool_calls"]


async def test_include_usage_toggles_usage_frames() -> None:
    with_usage = await _stream("mock/happy-path", _history(0), include_usage=True)
    without = await _stream("mock/happy-path", _history(0), include_usage=False)
    assert _usage_chunks(with_usage), "include_usage should emit a usage frame"
    assert not _usage_chunks(without), "usage must be withheld when not requested"


async def test_unknown_model_falls_back_to_happy_path() -> None:
    calls = _tool_calls(await _stream("mock/does-not-exist", _history(0)))
    assert [c["function"]["name"] for c in calls] == ["read_file"]


async def test_openai_route_prefix_is_tolerated() -> None:
    # LiteLLM may leave an "openai/" route prefix on the model name.
    calls = _tool_calls(await _stream("openai/mock/repair-loop", _history(0)))
    assert [c["function"]["name"] for c in calls] == ["write_file"]


async def test_non_streaming_completion_body_is_well_formed() -> None:
    transport = httpx.ASGITransport(app=create_app())
    async with httpx.AsyncClient(transport=transport, base_url="http://mock") as client:
        resp = await client.post(
            "/v1/chat/completions",
            json={"model": "mock/happy-path", "messages": _history(0), "stream": False},
        )
    body = resp.json()
    assert body["object"] == "chat.completion"
    assert body["choices"][0]["message"]["tool_calls"][0]["function"]["name"] == "read_file"
    assert body["usage"]["total_tokens"] > 0
