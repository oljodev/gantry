"""Provider-type → LiteLLM model-string mapping (pure, no DB)."""

from __future__ import annotations

import pytest

from gantry.core.models import ProviderType
from gantry.providers import litellm_model_string


@pytest.mark.parametrize(
    ("provider_type", "model", "expected"),
    [
        (ProviderType.OPENAI, "gpt-5", "openai/gpt-5"),
        (ProviderType.ANTHROPIC, "claude-opus-4-8", "anthropic/claude-opus-4-8"),
        (ProviderType.GOOGLE, "gemini-2.5-pro", "gemini/gemini-2.5-pro"),
        (ProviderType.XAI, "grok-4", "xai/grok-4"),
        (ProviderType.OPENROUTER, "qwen/qwen3-coder", "openrouter/qwen/qwen3-coder"),
        (ProviderType.LOCAL, "qwen3:32b", "openai/qwen3:32b"),
    ],
)
def test_prefixes(provider_type: ProviderType, model: str, expected: str) -> None:
    assert litellm_model_string(provider_type, model) == expected


def test_every_provider_type_is_mapped() -> None:
    # A missing entry raises KeyError; this guards against adding a provider
    # type without a route prefix.
    for provider_type in ProviderType:
        assert litellm_model_string(provider_type, "m").endswith("/m")


def test_already_prefixed_passes_through() -> None:
    assert litellm_model_string(ProviderType.XAI, "xai/grok-4") == "xai/grok-4"
