"""Model capability registry: which modalities a model slug can actually ingest.

The registry is the guard that stands between an operator attaching a mockup and
a swarm routing that mockup to DeepSeek-R1. It answers exactly one question —
*can this model read this kind of file?* — and it answers it from the model slug
alone, because that is all the worker has at claim time (the payload carries a
LiteLLM string, never a provider handle).

Matching is a deterministic, ordered scan of substring rules over the lowercased
slug, first match wins. Slugs arrive route-prefixed and sometimes double-vendored
(``openrouter/anthropic/claude-sonnet-4-5``), so matching on the WHOLE slug is
what makes ``openrouter/qwen/qwen3-coder`` and ``qwen3-coder`` classify
identically.

The default for an unknown slug is **text-only**, and that asymmetry is
deliberate: guessing "text-only" for a vision model costs one cheap transcription
pre-pass, while guessing "vision" for a text-only model is a hard provider 400
mid-run. Under-claiming degrades; over-claiming breaks.
"""

from __future__ import annotations

import enum
from collections.abc import Iterable


class Modality(enum.StrEnum):
    """An input kind a model may or may not accept."""

    TEXT = "text"
    IMAGE = "image"
    AUDIO = "audio"
    VIDEO = "video"
    PDF = "pdf"


TEXT_ONLY = frozenset({Modality.TEXT})
VISION = frozenset({Modality.TEXT, Modality.IMAGE})
#: Claude reads images and takes PDFs as native document blocks.
VISION_PDF = frozenset({Modality.TEXT, Modality.IMAGE, Modality.PDF})
#: Gemini is the one widely-served family that ingests audio and video directly.
OMNI = frozenset({Modality.TEXT, Modality.IMAGE, Modality.AUDIO, Modality.VIDEO, Modality.PDF})

#: Ordered ``(required substrings, modalities)`` rules — ALL substrings of a rule
#: must appear in the slug, and the FIRST matching rule wins. Order therefore
#: encodes specificity: the text-only exceptions of a vision family (``gpt-3.5``
#: under ``gpt-``) come before the family rule, and multi-token rules
#: (``qwen`` + ``vl``) come before their single-token family.
_RULES: tuple[tuple[tuple[str, ...], frozenset[Modality]], ...] = (
    # --- text-only exceptions inside otherwise-multimodal families -----------
    (("gpt-3.5",), TEXT_ONLY),
    (("o1-mini",), TEXT_ONLY),
    (("o1-preview",), TEXT_ONLY),
    # --- Anthropic ----------------------------------------------------------
    (("claude",), VISION_PDF),
    # --- Google -------------------------------------------------------------
    (("gemini",), OMNI),
    # --- OpenAI -------------------------------------------------------------
    (("gpt-4o",), VISION),
    (("gpt-4.1",), VISION),
    (("gpt-4-turbo",), VISION),
    (("gpt-4-vision",), VISION),
    (("gpt-5",), VISION),
    (("o3",), VISION),
    (("o4-mini",), VISION),
    # --- open-weight vision models ------------------------------------------
    (("pixtral",), VISION),
    (("llava",), VISION),
    (("internvl",), VISION),
    (("moondream",), VISION),
    (("qwen", "vl"), VISION),
    (("llama", "vision"), VISION),
    (("llama-4",), VISION),
    (("grok", "vision"), VISION),
    (("grok-4",), VISION),
    # --- text-only families, listed explicitly so the registry documents the
    # --- models a swarm actually routes code work to (same as the default).
    (("deepseek",), TEXT_ONLY),
    (("qwen",), TEXT_ONLY),
    (("codestral",), TEXT_ONLY),
    (("mistral",), TEXT_ONLY),
    (("mixtral",), TEXT_ONLY),
    (("kimi",), TEXT_ONLY),
)


def normalize_slug(model: str) -> str:
    """Lowercase, whitespace-stripped slug — the form the rules match against."""
    return model.strip().lower()


def capabilities_for(model: str) -> frozenset[Modality]:
    """Modalities ``model`` accepts. Unknown slugs are treated as text-only (see
    the module docstring for why that asymmetry is the safe one)."""
    slug = normalize_slug(model)
    if not slug:
        return TEXT_ONLY
    for needles, modalities in _RULES:
        if all(needle in slug for needle in needles):
            return modalities
    return TEXT_ONLY


def supports(model: str, modality: Modality) -> bool:
    """Whether ``model`` can ingest ``modality`` directly."""
    return modality in capabilities_for(model)


def missing_modalities(model: str, modalities: Iterable[Modality]) -> frozenset[Modality]:
    """The subset of ``modalities`` ``model`` CANNOT read — i.e. exactly what a
    pre-pass has to turn into text before the model ever sees the prompt."""
    caps = capabilities_for(model)
    return frozenset(m for m in modalities if m not in caps)
