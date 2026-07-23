"""Prompt-cache breakpoint insertion — pure, no litellm and no network.

These verify the transformation the loop's append-only history relies on to be
re-read at Anthropic cache rates instead of full input price every step.
"""

from __future__ import annotations

from gantry.runtime.llm import Message, with_cache_control

CLAUDE = "anthropic/claude-opus-4-8"


def _cc(message: Message) -> dict[str, object] | None:
    content = message.get("content")
    if isinstance(content, list) and content and isinstance(content[-1], dict):
        return content[-1].get("cache_control")
    return None


def test_marks_system_and_last_message() -> None:
    messages: list[Message] = [
        {"role": "system", "content": "SYS"},
        {"role": "user", "content": "GOAL"},
        {"role": "assistant", "content": "ok", "tool_calls": []},
        {"role": "tool", "tool_call_id": "c1", "content": "RESULT"},
    ]
    out = with_cache_control(messages, CLAUDE)

    # Static prefix (system) and rolling tail (last message) both get a breakpoint.
    assert _cc(out[0]) == {"type": "ephemeral"}
    assert _cc(out[-1]) == {"type": "ephemeral"}
    # The middle stays untouched (plain string content).
    assert out[1]["content"] == "GOAL"
    assert out[2]["content"] == "ok"


def test_promotes_string_body_to_block_preserving_text() -> None:
    out = with_cache_control([{"role": "system", "content": "SYS"}], CLAUDE)
    assert out[0]["content"] == [
        {"type": "text", "text": "SYS", "cache_control": {"type": "ephemeral"}}
    ]


def test_no_marks_for_non_anthropic_model() -> None:
    messages: list[Message] = [
        {"role": "system", "content": "SYS"},
        {"role": "user", "content": "GOAL"},
    ]
    out = with_cache_control(messages, "openai/gpt-5")
    assert out == messages
    assert all(isinstance(m["content"], str) for m in out)


def test_does_not_mutate_input() -> None:
    messages: list[Message] = [
        {"role": "system", "content": "SYS"},
        {"role": "user", "content": "GOAL"},
    ]
    with_cache_control(messages, CLAUDE)
    assert messages[0]["content"] == "SYS"  # original untouched
    assert messages[1]["content"] == "GOAL"


def test_empty_and_single_message_are_safe() -> None:
    assert with_cache_control([], CLAUDE) == []
    single: list[Message] = [{"role": "system", "content": "SYS"}]
    out = with_cache_control(single, CLAUDE)
    # System still marked; no second breakpoint when there is only one message.
    assert _cc(out[0]) == {"type": "ephemeral"}


def test_skips_uncacheable_tail_without_error() -> None:
    # An assistant turn that is only tool_calls (content=None) can't hold a
    # text breakpoint — it must pass through untouched rather than crash.
    messages: list[Message] = [
        {"role": "system", "content": "SYS"},
        {"role": "assistant", "content": None, "tool_calls": [{"id": "c1"}]},
    ]
    out = with_cache_control(messages, CLAUDE)
    assert out[-1]["content"] is None
    assert _cc(out[0]) == {"type": "ephemeral"}
