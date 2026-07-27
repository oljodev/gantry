"""Provider-type → LiteLLM model-string mapping.

The mapping happens once, at payload-build time (task create / team launch /
profile snapshot), so ``payload["model"]`` is always a final LiteLLM string
and the runtime loop never needs to know providers exist. The payload carries
only ``provider_id``; the worker resolves it to an API key (vault) and
``base_url`` when constructing the LLM client for the task.
"""

from __future__ import annotations

from gantry.core.models import Provider, ProviderType

#: LiteLLM route prefix per provider family. ``local`` is any
#: OpenAI-compatible server (ollama, vllm, llama.cpp, ...) addressed via
#: ``api_base``, which LiteLLM routes with the ``openai/`` prefix.
_PREFIX: dict[ProviderType, str] = {
    ProviderType.OPENAI: "openai",
    ProviderType.ANTHROPIC: "anthropic",
    ProviderType.GOOGLE: "gemini",
    ProviderType.XAI: "xai",
    ProviderType.OPENROUTER: "openrouter",
    ProviderType.LOCAL: "openai",
}


def litellm_model_string(provider_type: ProviderType, model: str) -> str:
    """``("google", "gemini-2.5-pro")`` → ``"gemini/gemini-2.5-pro"``.

    Strictly idempotent: the result always has EXACTLY ONE leading provider
    prefix. An already-prefixed name passes through unchanged, and — critically —
    a slug that arrived double-prefixed (``openrouter/openrouter/qwen/...``, which
    a small model can echo when it copies an already-mapped slug into a spawn) is
    collapsed back to a single prefix instead of being sent to LiteLLM and 400ing
    as an invalid model ID. The inner vendor segment is preserved, so a legitimate
    ``openrouter/openai/gpt-4`` (OpenRouter routing to an OpenAI model) is kept.
    """
    marker = f"{_PREFIX[provider_type]}/"
    # Strip every repeated leading provider prefix, then add back exactly one.
    while model.startswith(marker):
        model = model[len(marker) :]
    return f"{marker}{model}"


def resolve_model(provider: Provider | None, model: str | None, default: str) -> str:
    """Final LiteLLM model string from (optional provider, optional raw name).

    - provider + model  -> prefixed model
    - provider only     -> prefixed provider.default_model
    - model only        -> used verbatim (assumed already a LiteLLM string)
    - neither           -> the given default (settings.default_model)
    """
    if provider is not None:
        return litellm_model_string(provider.provider_type, model or provider.default_model)
    return model or default
