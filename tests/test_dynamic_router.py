"""The dynamic effective-cost router: the pure math (``runtime/routing.py``),
its live signal (``billing/routing_stats.py``'s rolling cache-hit-rate query
against real ``llm_usage_logs``), and its wiring into the worker
(``Worker._resolve_provider_routing`` -> ``LiteLLMClient.provider_routing`` ->
``extra_body["provider"]`` on the outbound request).

The two required scenarios (large prompt favours the high-cache-hit-rate
candidate; small prompt favours the cheap no-cache candidate) are proven
directly from the formula, with fixed, documented prices — nothing tuned
per-test to force an answer.
"""

from __future__ import annotations

import inspect
import uuid
from decimal import Decimal
from pathlib import Path

import pytest
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.billing.routing_stats import rolling_cache_hit_rate
from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.models import DEFAULT_WORKSPACE_ID, LlmUsageLog, Task, TaskKind
from gantry.runtime import routing
from gantry.runtime.llm import _request_kwargs
from gantry.runtime.routing import (
    ProviderCandidate,
    best_candidate,
    effective_input_cost_per_mtok,
    expected_total_cost_usd,
    provider_routing_payload,
    rank_candidates,
)
from gantry.worker.service import Worker, WorkerConfig

Sessions = async_sessionmaker[AsyncSession]

# --- the two candidates the spec's test cases are built from -----------------
#
# CACHING: a pricier nominal input rate, but a strong observed cache hit rate
# AND a pricier output rate — a realistic shape (a premium backend with prompt
# caching often isn't the cheapest on output either).
# NO_CACHE: a cheaper nominal input rate, no caching at all, and a cheaper
# output rate too.
CACHING = ProviderCandidate(
    name="caching-backend",
    price_in_per_mtok=Decimal("0.50"),
    price_cache_per_mtok=Decimal("0.10"),  # 20% of P_in, within the stated 10-25% band
    price_out_per_mtok=Decimal("3.00"),
    cache_hit_rate=Decimal("0.85"),
)
NO_CACHE = ProviderCandidate(
    name="no-cache-backend",
    price_in_per_mtok=Decimal("0.30"),
    price_cache_per_mtok=Decimal("0.30"),  # irrelevant at H=0, set equal for tidiness
    price_out_per_mtok=Decimal("1.20"),
    cache_hit_rate=Decimal("0.0"),
)
#: Held constant across both test cases — only T_in changes, exactly as specified.
OUTPUT_TOKENS = 1000


# --- the mathematical formula, in isolation -----------------------------------


def test_effective_input_cost_blends_by_hit_rate() -> None:
    # C_eff_in = (1 - H) * P_in + H * P_cache = 0.15*0.50 + 0.85*0.10 = 0.16
    assert effective_input_cost_per_mtok(CACHING) == Decimal("0.16")
    # H = 0 collapses to the plain input rate, regardless of the cache price.
    assert effective_input_cost_per_mtok(NO_CACHE) == Decimal("0.30")


def test_zero_hit_rate_ignores_the_cache_price_entirely() -> None:
    a = ProviderCandidate("a", Decimal("1"), Decimal("999"), Decimal("1"), Decimal("0"))
    b = ProviderCandidate("b", Decimal("1"), Decimal("0"), Decimal("1"), Decimal("0"))
    assert effective_input_cost_per_mtok(a) == effective_input_cost_per_mtok(b) == Decimal("1")


def test_expected_total_cost_matches_the_formula_by_hand() -> None:
    # (30000 * 0.16 + 1000 * 3.00) / 1e6
    assert expected_total_cost_usd(CACHING, 30_000, 1_000) == Decimal("0.0078")
    # (30000 * 0.30 + 1000 * 1.20) / 1e6
    assert expected_total_cost_usd(NO_CACHE, 30_000, 1_000) == Decimal("0.0102")


def test_negative_token_counts_are_rejected() -> None:
    with pytest.raises(ValueError, match="non-negative"):
        expected_total_cost_usd(CACHING, -1, 0)


@pytest.mark.parametrize(
    "kwargs",
    [
        {"cache_hit_rate": Decimal("1.5")},
        {"cache_hit_rate": Decimal("-0.1")},
        {"price_in_per_mtok": Decimal("-1")},
        {"name": ""},
    ],
)
def test_a_malformed_candidate_is_rejected_at_construction(kwargs: dict[str, object]) -> None:
    base = {
        "name": "x",
        "price_in_per_mtok": Decimal("1"),
        "price_cache_per_mtok": Decimal("0.1"),
        "price_out_per_mtok": Decimal("1"),
        "cache_hit_rate": Decimal("0"),
    }
    with pytest.raises(ValueError):
        ProviderCandidate(**{**base, **kwargs})  # type: ignore[arg-type]


# --- the two scenarios the spec requires --------------------------------------


def test_case_a_a_large_prompt_favours_the_high_cache_hit_rate_candidate() -> None:
    """T_in = 30,000: the cache saving on 30k input tokens outweighs the
    caching backend's pricier output rate, even though its nominal input price
    is higher and its output price is worse."""
    winner = best_candidate([CACHING, NO_CACHE], 30_000, OUTPUT_TOKENS)
    assert winner is not None and winner.name == "caching-backend"

    ranked = rank_candidates([CACHING, NO_CACHE], 30_000, OUTPUT_TOKENS)
    assert [r.candidate.name for r in ranked] == ["caching-backend", "no-cache-backend"]
    assert ranked[0].expected_cost_usd < ranked[1].expected_cost_usd


def test_case_b_a_small_prompt_favours_the_cheaper_no_cache_candidate() -> None:
    """T_in = 500: with so little input to cache, the caching backend's saving
    on input is tiny (80 vs 150 GC-equivalent USD-millionths) and its worse
    output rate dominates — the plain cheaper-nominal-rate candidate wins."""
    winner = best_candidate([CACHING, NO_CACHE], 500, OUTPUT_TOKENS)
    assert winner is not None and winner.name == "no-cache-backend"

    ranked = rank_candidates([CACHING, NO_CACHE], 500, OUTPUT_TOKENS)
    assert [r.candidate.name for r in ranked] == ["no-cache-backend", "caching-backend"]


def test_the_crossover_is_exactly_where_the_two_cost_lines_intersect() -> None:
    """Cost is linear in T_in for a fixed T_out, so there is exactly one
    breakeven point — everything above it should rank CACHING first, and
    everything below it NO_CACHE first. Pins the two scenarios above to the
    same underlying line rather than two independently-tuned assertions."""

    def cost_at(c: ProviderCandidate, t_in: int) -> Decimal:
        return expected_total_cost_usd(c, t_in, OUTPUT_TOKENS)

    # Solve (T_in*0.16 + 1000*3.00) == (T_in*0.30 + 1000*1.20) for T_in.
    breakeven = (Decimal(1000) * (Decimal("3.00") - Decimal("1.20"))) / (
        Decimal("0.30") - Decimal("0.16")
    )
    just_above = int(breakeven) + 50
    just_below = int(breakeven) - 50
    assert cost_at(CACHING, just_above) < cost_at(NO_CACHE, just_above)
    assert cost_at(CACHING, just_below) > cost_at(NO_CACHE, just_below)


# --- ranking / payload plumbing -----------------------------------------------


def test_rank_candidates_is_stable_on_a_tie() -> None:
    a = ProviderCandidate("a", Decimal("1"), Decimal("1"), Decimal("1"), Decimal("0"))
    b = ProviderCandidate("b", Decimal("1"), Decimal("1"), Decimal("1"), Decimal("0"))
    assert [r.candidate.name for r in rank_candidates([a, b], 100, 10)] == ["a", "b"]
    assert [r.candidate.name for r in rank_candidates([b, a], 100, 10)] == ["b", "a"]


def test_provider_routing_payload_orders_cheapest_first_and_allows_fallback() -> None:
    payload = provider_routing_payload([NO_CACHE, CACHING], 30_000, OUTPUT_TOKENS)
    assert payload == {"order": ["caching-backend", "no-cache-backend"], "allow_fallbacks": True}


def test_provider_routing_payload_is_none_for_no_candidates() -> None:
    assert provider_routing_payload([], 1000, 100) is None
    assert best_candidate([], 1000, 100) is None
    assert rank_candidates([], 1000, 100) == []


def test_allow_fallbacks_can_be_disabled() -> None:
    payload = provider_routing_payload([NO_CACHE], 100, 10, allow_fallbacks=False)
    assert payload == {"order": ["no-cache-backend"], "allow_fallbacks": False}


# --- zero hardcoded provider names --------------------------------------------

#: Real backend/vendor names the task explicitly forbids hardcoding into the
#: decision engine. Deliberately excludes "openrouter", which the module
#: legitimately names as the ROUTING PROTOCOL it targets, not a backend choice.
_FORBIDDEN_VENDOR_NAMES = (
    "digitalocean",
    "alibaba",
    "together",
    "fireworks",
    "deepinfra",
    "novita",
    "openai",
    "anthropic",
    "google",
    "azure",
    "groq",
    "cerebras",
    "mistral",
    "deepseek",
    "qwen",
)


def test_the_calculation_engine_hardcodes_no_provider_names() -> None:
    source = Path(inspect.getfile(routing)).read_text().lower()
    hits = [name for name in _FORBIDDEN_VENDOR_NAMES if name in source]
    assert hits == [], f"routing.py must not hardcode provider names, found: {hits}"


def test_the_engine_never_branches_on_a_candidate_name() -> None:
    """A structural check to back the source-scan above: renaming every
    candidate must not change the ranking — the engine reads ``name`` only to
    echo it back, never to decide anything."""
    renamed_caching = ProviderCandidate(
        "anything-a",
        CACHING.price_in_per_mtok,
        CACHING.price_cache_per_mtok,
        CACHING.price_out_per_mtok,
        CACHING.cache_hit_rate,
    )
    renamed_no_cache = ProviderCandidate(
        "anything-b",
        NO_CACHE.price_in_per_mtok,
        NO_CACHE.price_cache_per_mtok,
        NO_CACHE.price_out_per_mtok,
        NO_CACHE.cache_hit_rate,
    )
    winner = best_candidate([renamed_caching, renamed_no_cache], 30_000, OUTPUT_TOKENS)
    assert winner is not None and winner.name == "anything-a"


# --- the request actually carries the preference ------------------------------


def test_request_kwargs_carries_the_provider_order_as_extra_body() -> None:
    payload = provider_routing_payload([NO_CACHE, CACHING], 30_000, OUTPUT_TOKENS)
    kwargs = _request_kwargs("m", [], (), None, None, None, payload)
    assert kwargs["extra_body"] == {"provider": payload}


def test_request_kwargs_omits_extra_body_with_no_routing_preference() -> None:
    kwargs = _request_kwargs("m", [], (), None, None, None, None)
    assert "extra_body" not in kwargs


# --- the learning half: rolling H_k from real llm_usage_logs ------------------

MODEL = "fake/router-e2e-model"


async def _log_call(
    db: Sessions, *, model: str = MODEL, prompt_tokens: int, cache_read_tokens: int
) -> None:
    async with session_scope(db) as session:
        session.add(
            LlmUsageLog(
                id=uuid.uuid4(),
                workspace_id=DEFAULT_WORKSPACE_ID,
                model_slug=model,
                prompt_tokens=prompt_tokens,
                completion_tokens=1,
                total_tokens=prompt_tokens + 1,
                cache_read_tokens=cache_read_tokens,
            )
        )


async def test_rolling_hit_rate_is_none_with_no_history(db: Sessions) -> None:
    assert await _rate(db, "fake/never-called") is None


async def test_rolling_hit_rate_is_the_ratio_of_cache_reads_to_prompt_tokens(
    db: Sessions,
) -> None:
    await _log_call(db, prompt_tokens=1000, cache_read_tokens=800)
    await _log_call(db, prompt_tokens=1000, cache_read_tokens=900)
    # (800 + 900) / (1000 + 1000)
    assert await _rate(db, MODEL) == Decimal("0.85")


async def test_rolling_hit_rate_only_looks_at_the_most_recent_window(db: Sessions) -> None:
    # An old, cache-cold run of calls the window should age out...
    for _ in range(5):
        await _log_call(db, prompt_tokens=1000, cache_read_tokens=0)
    # ...followed by a provider that turned caching on.
    for _ in range(3):
        await _log_call(db, prompt_tokens=1000, cache_read_tokens=1000)
    async with db() as session:
        windowed = await rolling_cache_hit_rate(session, MODEL, limit=3)
    assert windowed == Decimal("1")  # only the 3 warm calls are in the window


async def _rate(db: Sessions, model: str) -> Decimal | None:
    async with db() as session:
        return await rolling_cache_hit_rate(session, model)


# --- wired into the worker ----------------------------------------------------


async def _task_with_payload(db: Sessions, payload: dict[str, object]) -> Task:
    async with session_scope(db) as session:
        return await queue.enqueue(
            session, workspace_id=DEFAULT_WORKSPACE_ID, kind=TaskKind.EXECUTE, payload=payload
        )


def _worker(db: Sessions, tmp_path: Path) -> Worker:
    from .fakes import ScriptedLLM

    config = WorkerConfig(worker_id="router-test-worker", workspace_root=tmp_path / "ws")
    return Worker(db, config, ScriptedLLM([]))


async def test_a_task_with_no_candidates_resolves_no_routing_preference(
    db: Sessions, tmp_path: Path
) -> None:
    worker = _worker(db, tmp_path)
    task = await _task_with_payload(db, {"goal": "plain task"})
    async with db() as session:
        assert await worker._resolve_provider_routing(session, task) is None


async def test_a_task_with_candidates_resolves_the_cheapest_first_order(
    db: Sessions, tmp_path: Path
) -> None:
    worker = _worker(db, tmp_path)
    task = await _task_with_payload(
        db,
        {
            "goal": "x" * 400_000,  # ~100k-token prompt: large enough to favour caching
            "model": "fake/router-worker-model-a",
            "provider_candidates": [
                {
                    "name": "caching-backend",
                    "price_in_per_mtok": "0.50",
                    "price_cache_per_mtok": "0.10",
                    "price_out_per_mtok": "3.00",
                    "cache_hit_rate": "0.85",
                },
                {
                    "name": "no-cache-backend",
                    "price_in_per_mtok": "0.30",
                    "price_out_per_mtok": "1.20",
                    "cache_hit_rate": "0.0",
                },
            ],
        },
    )
    async with db() as session:
        routing_payload = await worker._resolve_provider_routing(session, task)
    assert routing_payload == {
        "order": ["caching-backend", "no-cache-backend"],
        "allow_fallbacks": True,
    }


async def test_a_candidate_without_an_explicit_hit_rate_uses_the_rolling_db_average(
    db: Sessions, tmp_path: Path
) -> None:
    model = "fake/router-worker-model-b"
    await _log_call(db, model=model, prompt_tokens=1000, cache_read_tokens=900)  # H ~= 0.9
    worker = _worker(db, tmp_path)
    task = await _task_with_payload(
        db,
        {
            "goal": "x" * 400_000,
            "model": model,
            "provider_candidates": [
                # No cache_hit_rate given -> falls back to the observed ~0.9.
                {
                    "name": "learned-backend",
                    "price_in_per_mtok": "0.50",
                    "price_cache_per_mtok": "0.10",
                    "price_out_per_mtok": "3.00",
                },
                {
                    "name": "no-cache-backend",
                    "price_in_per_mtok": "0.30",
                    "price_out_per_mtok": "1.20",
                    "cache_hit_rate": "0.0",
                },
            ],
        },
    )
    async with db() as session:
        routing_payload = await worker._resolve_provider_routing(session, task)
    assert routing_payload is not None
    assert routing_payload["order"][0] == "learned-backend"


async def test_a_malformed_candidate_entry_is_skipped_not_crashing(
    db: Sessions, tmp_path: Path
) -> None:
    worker = _worker(db, tmp_path)
    task = await _task_with_payload(
        db,
        {
            "goal": "hello",
            "provider_candidates": [
                {"name": "broken", "price_in_per_mtok": "not-a-number", "price_out_per_mtok": "1"},
                "not-even-a-dict",
            ],
        },
    )
    async with db() as session:
        assert await worker._resolve_provider_routing(session, task) is None
