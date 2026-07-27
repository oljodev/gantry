"""Provider routing enforcement: every provider-backed model call must go through
its provider in LiteLLM. Regression for a spawned DeepSeek worker that failed auth
because a bare ``deepseek/deepseek-r1`` slug was dispatched DIRECTLY to DeepSeek
with the leader's OpenRouter key instead of being routed ``openrouter/...``.
"""

from __future__ import annotations

from gantry.core.models import DEFAULT_WORKSPACE_ID, Provider, ProviderType, Task, TaskKind
from gantry.providers import litellm_model_string
from gantry.worker.service import _route_task_model


def _openrouter() -> Provider:
    return Provider(
        workspace_id=DEFAULT_WORKSPACE_ID,
        name="OpenRouter",
        provider_type=ProviderType.OPENROUTER,
        base_url="https://openrouter.ai/api/v1",
        default_model="deepseek/deepseek-chat",
    )


def _task(model: str | None) -> Task:
    payload: dict[str, object] = {"goal": "x"}
    if model is not None:
        payload["model"] = model
    return Task(kind=TaskKind.EXECUTE, payload=payload)


def test_litellm_string_prefixes_a_bare_openrouter_slug() -> None:
    # The exact failure: a bare vendor slug must be routed through OpenRouter, not
    # sent to the vendor directly.
    assert (
        litellm_model_string(ProviderType.OPENROUTER, "deepseek/deepseek-r1")
        == "openrouter/deepseek/deepseek-r1"
    )
    assert litellm_model_string(ProviderType.OPENROUTER, "qwen/qwen3-coder") == (
        "openrouter/qwen/qwen3-coder"
    )


def test_route_task_model_normalizes_a_bare_child_slug() -> None:
    # A child inherited an OpenRouter provider but a bare 'deepseek/deepseek-r1'
    # slug the leader picked; routing must prefix it so no direct DeepSeek call is made.
    task = _task("deepseek/deepseek-r1")
    _route_task_model(task, _openrouter())
    assert task.payload["model"] == "openrouter/deepseek/deepseek-r1"


def test_route_task_model_is_idempotent_for_an_already_routed_slug() -> None:
    task = _task("openrouter/deepseek/deepseek-r1")
    _route_task_model(task, _openrouter())
    assert task.payload["model"] == "openrouter/deepseek/deepseek-r1"


def test_route_task_model_leaves_provider_less_tasks_verbatim() -> None:
    # No provider -> the env-default client is used; the model is untouched.
    task = _task("deepseek/deepseek-r1")
    _route_task_model(task, None)
    assert task.payload["model"] == "deepseek/deepseek-r1"


def test_route_task_model_handles_a_missing_model() -> None:
    task = _task(None)
    _route_task_model(task, _openrouter())  # must not raise or invent a model
    assert "model" not in task.payload
