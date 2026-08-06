"""Turning attachment snapshots into the agent's opening user message.

Pure prompt assembly: given the goal text and the payload's attachment
snapshots, produce the OpenAI-style message the loop starts from. No I/O, no
DB — which is what lets ``state.initial_messages`` call it during rehydration
and get a byte-identical message every time, live or resumed.

The shape depends on what the worker's model can read (decided upstream, in
``prepare``):

- text and PDF attachments become fenced text sections, so a text-only model
  ingests the document directly;
- an image the model CAN see becomes an ``image_url`` block appended after the
  text, promoting the message to block form;
- an image it cannot see becomes a transcription section instead — the pre-pass
  already turned it into words, and the model never learns an image existed in a
  form it could not use.
"""

from __future__ import annotations

from collections.abc import Sequence
from typing import Any

from gantry.runtime.llm import Message

#: Fence long enough that fenced code inside an attachment can't close it early.
_FENCE = "````"


def _human_size(size_bytes: int) -> str:
    if size_bytes < 1024:
        return f"{size_bytes} B"
    if size_bytes < 1024 * 1024:
        return f"{size_bytes / 1024:.1f} KB"
    return f"{size_bytes / (1024 * 1024):.1f} MB"


def _header(index: int, entry: dict[str, Any]) -> str:
    filename = str(entry.get("filename") or "attachment")
    media_type = str(entry.get("media_type") or "application/octet-stream")
    parts = [media_type, _human_size(int(entry.get("size_bytes") or 0))]
    pages = int(entry.get("pages") or 0)
    if pages:
        parts.append(f"{pages} pages")
    return f"### {index}. {filename} ({', '.join(parts)})"


def describe(index: int, entry: dict[str, Any]) -> str:
    """The text section for one attachment: header plus whatever content the
    model can actually read (extracted text, a transcription, or an explicit
    note that the file could not be read — never a silent omission)."""
    lines = [_header(index, entry)]
    text = str(entry.get("text") or "").strip()
    transcript = str(entry.get("transcript") or "").strip()
    note = str(entry.get("note") or "").strip()
    error = str(entry.get("extract_error") or "").strip()

    if text:
        lines.append(f"{_FENCE}\n{text}\n{_FENCE}")
    if transcript:
        model = str(entry.get("transcript_model") or "a vision model")
        lines.append(
            f"Description of this file, transcribed by {model} because your model "
            f"cannot read it directly:\n{_FENCE}\n{transcript}\n{_FENCE}"
        )
    if entry.get("data_url") and not text and not transcript:
        lines.append("(attached below — read it directly)")
    if error:
        lines.append(f"[gantry: could not extract text — {error}]")
    if note:
        lines.append(f"[gantry: {note}]")
    if len(lines) == 1:
        lines.append("[gantry: this file's contents could not be included]")
    return "\n\n".join(lines)


def attachments_text(attachments: Sequence[dict[str, Any]]) -> str:
    """The whole '## Attached files' appendix, or "" when there is nothing to say."""
    if not attachments:
        return ""
    sections = [describe(index, entry) for index, entry in enumerate(attachments, 1)]
    return (
        "\n\n## Attached files\n\n"
        "The user attached the following file(s) to this task. Treat them as part "
        "of the specification.\n\n" + "\n\n".join(sections)
    )


def image_blocks(attachments: Sequence[dict[str, Any]]) -> list[dict[str, Any]]:
    """Provider image blocks for the attachments the model can see natively —
    empty for a text-only worker, which is the whole point of the guard."""
    return [
        {"type": "image_url", "image_url": {"url": str(entry["data_url"])}}
        for entry in attachments
        if entry.get("data_url")
    ]


def goal_message(goal: str, attachments: Sequence[dict[str, Any]]) -> Message:
    """The agent's opening user message.

    Stays a plain string when there is nothing visual to send (the overwhelming
    majority of runs, and the form every provider handles most cheaply) and is
    promoted to block form only when real image blocks are attached.
    """
    text = f"{goal}{attachments_text(attachments)}"
    blocks = image_blocks(attachments)
    if not blocks:
        return {"role": "user", "content": text}
    return {"role": "user", "content": [{"type": "text", "text": text}, *blocks]}
