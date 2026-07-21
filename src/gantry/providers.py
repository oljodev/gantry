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

    Already-prefixed model names pass through untouched so users can paste
    full LiteLLM strings anywhere a bare model name is accepted.
    """
    prefix = _PREFIX[provider_type]
    if model.startswith(f"{prefix}/"):
        return model
    return f"{prefix}/{model}"


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
