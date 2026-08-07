"""Postgres cannot store a NUL byte (``\\x00``) in TEXT or JSONB — not even
escaped. asyncpg raises it as ``UntranslatableCharacterError`` (or, coming from
a driver default-decoded string, ``DataError: invalid message format``); either
way the write is rejected and, unhandled, takes the caller's whole transaction
down with it.

That is a real, reachable condition, not a hypothetical: a `bash` tool call is
model-authored and unconstrained (``cat`` a binary file, ``git diff`` a binary
blob), its stdout/stderr is decoded with ``errors="replace"`` — which passes a
literal ``\\x00`` byte straight through unchanged — and streamed verbatim into
`terminal_chunk` events. LLM content, tool results, and diagnostics can carry
the same byte for the same reason: they are text *decoded from* an untrusted
byte stream, not text a human typed.

Rather than audit and patch every producer, every string that is about to cross
into a TEXT or JSONB column goes through here first — once, at the write
boundary (``append_event``, ``queue.complete``/``fail``, the attachment text/
transcript path). A producer that is fixed later is still safe; a new producer
that forgets to sanitize is still safe. Only ``\\x00`` is touched — this is not
a general control-character or ANSI-escape scrubber, and must never become one:
the terminal pane renders real ANSI colour codes, and a tab or a legitimate
control byte in test output is not this bug.
"""

from __future__ import annotations

from typing import Any

_NUL = "\x00"


def strip_null_bytes(text: str) -> str:
    """Remove embedded NUL bytes from one string. A no-op (same object) for the
    overwhelming common case of a string that has none, so this is cheap enough
    to call unconditionally at every write boundary."""
    return text.replace(_NUL, "") if _NUL in text else text


def sanitize_json(value: Any) -> Any:
    """Recursively strip NUL bytes from every string inside a JSON-shaped value
    (the event/result payloads that go into JSONB columns).

    Walks dict/list/tuple; every other type (int, float, bool, None, and
    anything JSON can't represent anyway) passes through untouched. Safe to
    call on ``None`` or an already-clean payload — it will simply hand back
    the same shape.
    """
    if isinstance(value, str):
        return strip_null_bytes(value)
    if isinstance(value, dict):
        return {key: sanitize_json(item) for key, item in value.items()}
    if isinstance(value, list):
        return [sanitize_json(item) for item in value]
    if isinstance(value, tuple):
        return tuple(sanitize_json(item) for item in value)
    return value
