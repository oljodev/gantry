"""The vision pre-pass: turning a file a worker cannot see into words it can.

When the capability guard finds that a prompt carries an image (or video) but
the worker runs a text-only model — a DeepSeek-R1 planner, a Qwen coder — the
image is not dropped and the run does not fail. One cheap call to a vision model
transcribes it into detailed context text, and THAT text is what the worker
reads.

Two constraints shape the design:

- **The transcription must run on the run's own provider key.** A worker holds
  exactly one decrypted key, so the fallback vision model is derived from the
  worker model's LiteLLM route prefix (``openrouter/...`` -> an OpenRouter
  vision slug). An operator override (``GANTRY_VISION_MODEL``) wins.
- **The transcript must be durable.** It is persisted on the attachment row, so
  a resumed task reuses the identical text instead of paying for — and drifting
  towards — a second description. Same input, same prompt, same context.
"""

from __future__ import annotations

import base64

from gantry.attachments.capabilities import Modality, supports
from gantry.logging import get_logger
from gantry.runtime.llm import LLMClient, Message

logger = get_logger(__name__)

#: Fallback vision model per LiteLLM route prefix. The slug must be servable by
#: the SAME key the worker already holds, so the prefix is preserved.
DEFAULT_VISION_MODELS: dict[str, str] = {
    "anthropic": "anthropic/claude-haiku-4-5",
    "openai": "openai/gpt-4o",
    "gemini": "gemini/gemini-2.5-flash",
    "openrouter": "openrouter/anthropic/claude-3.5-sonnet",
}


def route_prefix(model: str) -> str:
    """The LiteLLM provider route of a model slug (``openrouter/qwen/x`` ->
    ``openrouter``), or "" for an unprefixed name."""
    head, sep, _ = model.strip().lower().partition("/")
    return head if sep else ""


def default_vision_model(worker_model: str) -> str | None:
    """A fast vision model reachable with the worker's own credentials, or None
    when we can't name one for that provider (a `local` endpoint, say) — in which
    case the operator must set ``GANTRY_VISION_MODEL`` and the run says so."""
    return DEFAULT_VISION_MODELS.get(route_prefix(worker_model))


def resolve_vision_model(
    worker_model: str, configured: str | None, modality: Modality = Modality.IMAGE
) -> str | None:
    """The model that will do the transcription, or None if there isn't one.

    A configured override is used verbatim only if it can actually read the
    modality — pointing ``GANTRY_VISION_MODEL`` at a text-only slug must degrade
    to "cannot read this file", never to a provider 400 mid-run.
    """
    candidate = configured or default_vision_model(worker_model)
    if candidate and supports(candidate, modality):
        return candidate
    return None


TRANSCRIBE_SYSTEM_PROMPT = (
    "You transcribe visual material for another AI agent that CANNOT see images. "
    "Your description is the only thing it will ever know about this file, so it "
    "must stand on its own."
)

TRANSCRIBE_INSTRUCTION = (
    "Describe this file in complete, specific detail so an engineer who cannot see "
    "it could act on your description alone.\n\n"
    "- Transcribe ALL visible text verbatim (labels, buttons, code, numbers, axes, "
    "error messages), preserving its structure.\n"
    "- Describe layout and hierarchy concretely: what is where, sizes, spacing, "
    "alignment, grouping, and ordering.\n"
    "- Name colours (hex where you can tell), typography, borders, shadows, icons, "
    "and states (hover, disabled, selected, focused).\n"
    "- For diagrams and charts: state every node, edge, series, and data point you "
    "can read, plus the relationships between them.\n"
    "- For screenshots of code or terminals: reproduce the content exactly.\n\n"
    "Do not editorialize, do not summarize away detail, and do not guess at things "
    "you cannot see — say plainly when something is illegible."
)


def data_url(media_type: str, data: bytes) -> str:
    """A ``data:`` URL — how LiteLLM carries inline media to every provider."""
    return f"data:{media_type};base64,{base64.b64encode(data).decode('ascii')}"


def transcription_messages(
    media_type: str, data: bytes, *, filename: str = "", goal: str = ""
) -> list[Message]:
    """The two-message prompt for one transcription.

    The task goal rides along as context when we have it: "describe this image"
    and "describe this image; the worker must rebuild it as a React component"
    produce very different — and differently useful — transcriptions.
    """
    parts = [TRANSCRIBE_INSTRUCTION]
    if filename:
        parts.append(f"Filename: {filename}")
    if goal:
        parts.append(
            "For context, the engineer reading your description was given this task:\n"
            f"{goal.strip()[:2000]}"
        )
    return [
        {"role": "system", "content": TRANSCRIBE_SYSTEM_PROMPT},
        {
            "role": "user",
            "content": [
                {"type": "text", "text": "\n\n".join(parts)},
                {"type": "image_url", "image_url": {"url": data_url(media_type, data)}},
            ],
        },
    ]


async def transcribe(
    llm: LLMClient,
    model: str,
    *,
    media_type: str,
    data: bytes,
    filename: str = "",
    goal: str = "",
) -> str:
    """Describe one file with a vision model. Returns "" if the model answered
    with nothing; the caller turns that into an explicit note rather than
    pretending the attachment was delivered."""
    response = await llm.complete(
        model=model,
        messages=transcription_messages(media_type, data, filename=filename, goal=goal),
        tools=(),
    )
    text = (response.content or "").strip()
    logger.info(
        "attachments.transcribed",
        model=model,
        filename=filename,
        media_type=media_type,
        chars=len(text),
    )
    return text
