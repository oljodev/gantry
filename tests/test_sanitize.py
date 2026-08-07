"""strip_null_bytes / sanitize_json: the pure functions, in isolation."""

from __future__ import annotations

from gantry.core.sanitize import sanitize_json, strip_null_bytes


def test_strips_a_null_byte() -> None:
    assert strip_null_bytes("before\x00after") == "beforeafter"


def test_strips_multiple_null_bytes() -> None:
    assert strip_null_bytes("\x00a\x00b\x00") == "ab"


def test_leaves_a_clean_string_untouched() -> None:
    text = "ordinary output, no surprises here"
    assert strip_null_bytes(text) == text


def test_leaves_other_control_and_unicode_characters_alone() -> None:
    """This is a NUL-byte scrubber, not a general control-character filter — a
    tab, a newline, an ANSI escape, or an emoji must survive untouched. The
    terminal pane renders real ANSI colour codes; mangling those would be a
    second, self-inflicted bug."""
    text = "line1\nline2\ttabbed\x1b[31mred\x1b[0m \U0001f600"
    assert strip_null_bytes(text) == text


def test_returns_the_same_object_when_there_is_nothing_to_strip() -> None:
    # Not a hard requirement, but the fast path should be genuinely cheap: no
    # allocation for the overwhelming common case of a clean string.
    text = "no nulls here"
    assert strip_null_bytes(text) is text


def test_sanitize_json_strips_a_bare_string() -> None:
    assert sanitize_json("a\x00b") == "ab"


def test_sanitize_json_walks_nested_dicts_and_lists() -> None:
    payload = {
        "data": "chunk with a \x00 byte",
        "nested": {"deeper": ["clean", "dirty\x00er"]},
        "count": 3,
        "ok": True,
        "nothing": None,
    }
    assert sanitize_json(payload) == {
        "data": "chunk with a  byte",
        "nested": {"deeper": ["clean", "dirtyer"]},
        "count": 3,
        "ok": True,
        "nothing": None,
    }


def test_sanitize_json_walks_tuples() -> None:
    assert sanitize_json(("a\x00", "b")) == ("a", "b")


def test_sanitize_json_passes_non_string_scalars_through_unchanged() -> None:
    for value in (1, 1.5, True, False, None):
        assert sanitize_json(value) is value


def test_sanitize_json_handles_none_and_empty() -> None:
    assert sanitize_json(None) is None
    assert sanitize_json({}) == {}
    assert sanitize_json([]) == []
