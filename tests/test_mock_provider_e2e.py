"""End-to-end: drive the REAL agent loop, REAL tools, and REAL circuit breakers
against the live mock provider over a real socket — proving the breakers fire for
$0 of token spend. DB-backed (the `db` fixture), but no full WorkerService.
"""

from __future__ import annotations

import asyncio
import json
from pathlib import Path

import httpx
import pytest
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.models import TaskStatus
from gantry.core.queue import retry_backoff_seconds
from gantry.runtime.llm import LiteLLMClient
from gantry.runtime.loop import AgentLoopError, run_agent_task
from gantry.runtime.tools import TaskStalled
from gantry.worker.service import _is_permanent_provider_error
from gantry.worker.tools import build_coding_registry
from gantry.worker.tools.integrate import MergeChildBranchesTool, make_conflict_resolver

from .mock_provider.scenarios import (
    CORRUPTED_SSE_FAILURES,
    HAPPY_WRITE_BODY,
    HAPPY_WRITE_PATH,
    RATE_LIMIT_FAILURES,
)
from .mock_provider.server import running_server
from .test_agent_loop import enqueue_agent_task
from .test_staging_gate import _STAGING, _ctx, _leader, _remote_heads, _worker_pushes
from .test_worker_git import git, origin  # noqa: F401  (fixture re-export)

Sessions = async_sessionmaker[AsyncSession]


def _client(base_url: str) -> LiteLLMClient:
    # A dummy key satisfies the OpenAI route; prompt caching off (not Anthropic).
    return LiteLLMClient(api_key="mock-key", api_base=base_url, prompt_caching=False)


async def test_happy_path_completes_in_two_steps(db: Sessions, tmp_path: Path) -> None:
    (tmp_path / "input.txt").write_text("hello from the mock\n")
    async with running_server() as base_url:
        task = await enqueue_agent_task(db, {"model": "openai/mock/happy-path"})
        outcome = await run_agent_task(
            db, task, _client(base_url), build_coding_registry(), workspace=tmp_path
        )
    # The real write tool actually wrote the file in the workspace.
    assert (tmp_path / HAPPY_WRITE_PATH).read_text() == HAPPY_WRITE_BODY
    assert "two steps" in (outcome.final_text or "").lower()


async def test_repair_loop_trips_the_repair_wave_breaker(db: Sessions, tmp_path: Path) -> None:
    # The mock rewrites the same broken file each step; the real write-time AST
    # check emits the recurring diagnostic that escalates the task.
    async with running_server() as base_url:
        task = await enqueue_agent_task(db, {"model": "openai/mock/repair-loop"})
        with pytest.raises(TaskStalled):
            await run_agent_task(
                db, task, _client(base_url), build_coding_registry(), workspace=tmp_path
            )


async def test_budget_runaway_is_capped_by_the_step_limit(db: Sessions, tmp_path: Path) -> None:
    # Unbounded token wall + a tool call every step -> never finishes. Today the
    # finite step cap halts it; the mid-stream $-sentinel attaches here once the
    # stashed live-metering feature lands.
    async with running_server() as base_url:
        task = await enqueue_agent_task(db, {"model": "openai/mock/budget-runaway", "max_steps": 4})
        with pytest.raises(AgentLoopError):
            await run_agent_task(
                db, task, _client(base_url), build_coding_registry(), workspace=tmp_path
            )


# --- Agent-loop safeguards ----------------------------------------------------


async def test_hallucinated_tool_trips_the_repair_wave_breaker(
    db: Sessions, tmp_path: Path
) -> None:
    # Every step calls a tool that doesn't exist. The loop returns a structured
    # UnknownTool diagnostic with a stable fingerprint, so the repair-wave breaker
    # escalates (TaskStalled) instead of the agent burning its budget on a phantom.
    async with running_server() as base_url:
        task = await enqueue_agent_task(db, {"model": "openai/mock/hallucinated-tool"})
        with pytest.raises(TaskStalled):
            await run_agent_task(
                db, task, _client(base_url), build_coding_registry(), workspace=tmp_path
            )


async def test_passive_read_loop_is_broken(db: Sessions, tmp_path: Path) -> None:
    # Every step is a read-only list_dir with no edit ever. The passive-read
    # breaker fails the leaf worker once it crosses the read budget, well before
    # the step cap.
    async with running_server() as base_url:
        task = await enqueue_agent_task(db, {"model": "openai/mock/passive-read-loop"})
        with pytest.raises(AgentLoopError, match="passive read loop"):
            await run_agent_task(
                db, task, _client(base_url), build_coding_registry(), workspace=tmp_path
            )


# --- Git & integration chaos (merge layer, mock as the conflict resolver) ------


async def test_git_conflict_is_resolved_and_integrated(origin: Path, tmp_path: Path) -> None:  # noqa: F811
    # Two workers create the same file with colliding content -> an add/add
    # conflict. The mock stands in for Gantry's LLM conflict resolver and
    # union-merges; the staging gate then verifies (compileall) and publishes.
    branch_a = await _worker_pushes(origin, tmp_path, "a", {"values.py": "FROM_A = 2\n"})
    branch_b = await _worker_pushes(origin, tmp_path, "b", {"values.py": "FROM_B = 3\n"})
    leader = await _leader(origin, tmp_path)
    async with running_server() as base_url:
        resolver = make_conflict_resolver(_client(base_url), "openai/mock/git-conflict")
        tool = MergeChildBranchesTool(leader.auth, leader.branch or "main", resolver)
        result = await tool.execute(
            {"branches": [branch_a, branch_b], "into": _STAGING}, _ctx(leader)
        )
    assert not result.is_error, result.content
    summary = json.loads(result.content)
    assert summary["pushed"] is True
    assert branch_b in summary["auto_resolved"]
    # Both workers' edits survive in the integrated, published file.
    merged = git("--git-dir", str(origin), "show", f"{_STAGING}:values.py")
    assert "FROM_A = 2" in merged and "FROM_B = 3" in merged


async def test_missing_branch_blocks_even_a_partial_delivery(origin: Path, tmp_path: Path) -> None:  # noqa: F811
    # One worker delivered, one "succeeded" without pushing. The integration must
    # NOT be published, and the leader gets a structured MissingBranch diagnostic.
    delivered = await _worker_pushes(origin, tmp_path, "a", {"mod.py": "OK = 1\n"})
    leader = await _leader(origin, tmp_path)
    tool = MergeChildBranchesTool(leader.auth, leader.branch or "main")
    result = await tool.execute(
        {"branches": [delivered, "gantry/task-neverpushed"], "into": _STAGING}, _ctx(leader)
    )
    assert result.is_error
    assert result.diagnostics and result.diagnostics[0].code == "MissingBranch"
    assert "not on origin" in result.diagnostics[0].message
    assert _STAGING not in _remote_heads(leader)


# --- API & network resilience (queue layer: claim -> fail-with-backoff -> reclaim) --


async def _drive_until_settled(
    db: Sessions, base_url: str, model: str, workspace: Path, *, backoff_base: float
) -> tuple[str, int, object]:
    """Mimic the worker's real retry path against the live mock: claim, run, and on
    a retryable provider error re-queue with backoff and re-claim, until the task
    succeeds or fails terminally. Uses the REAL queue + the real retryable/permanent
    classification, so it exercises Gantry's actual resilience, not a stand-in."""
    task = await enqueue_agent_task(db, {"model": model})
    client = _client(base_url)
    attempts = 0
    while True:
        async with session_scope(db) as session:
            claimed = await queue.claim(session, worker_id="chaos-worker")
        assert claimed is not None, "task became unclaimable before settling"
        attempts += 1
        try:
            outcome = await run_agent_task(
                db, claimed, client, build_coding_registry(), workspace=workspace
            )
        except Exception as exc:  # the worker likewise catches provider errors broadly
            retryable = not _is_permanent_provider_error(exc)
            async with session_scope(db) as session:
                status = await queue.fail(
                    session,
                    task_id=claimed.id,
                    worker_id="chaos-worker",
                    attempt=claimed.attempt,
                    error=repr(exc),
                    retryable=retryable,
                    backoff_base_seconds=backoff_base,
                )
            if status is TaskStatus.FAILED:
                return "failed", attempts, task.id
            # Wait out the (tiny) backoff so the re-queued task becomes claimable.
            await asyncio.sleep(retry_backoff_seconds(claimed.attempt, backoff_base) + 0.02)
        else:
            async with session_scope(db) as session:
                await queue.complete(
                    session,
                    task_id=claimed.id,
                    worker_id="chaos-worker",
                    attempt=claimed.attempt,
                    result={"final_text": outcome.final_text or ""},
                    cost_usd=0.0,
                )
            return "succeeded", attempts, task.id


async def _served(base_url: str, model_key: str) -> int:
    """How many times the live mock has served ``model_key`` — proof the client
    actually retried through the injected failures."""
    root = base_url.removesuffix("/v1")
    async with httpx.AsyncClient() as client:
        resp = await client.get(f"{root}/counters")
    return int(resp.json().get(model_key, 0))


async def test_rate_limit_429_is_transparently_retried(db: Sessions, tmp_path: Path) -> None:
    # The provider client (LiteLLM) is Gantry's FIRST line of defense: it retries a
    # transient 429 with its own backoff, so the run recovers within a single task
    # attempt. Gantry's queue-level backoff is the second line, for when the client
    # exhausts its retries.
    async with running_server() as base_url:
        status, attempts, _ = await _drive_until_settled(
            db, base_url, "openai/mock/rate-limit-429", tmp_path, backoff_base=0.0
        )
        served = await _served(base_url, "mock/rate-limit-429")
    assert status == "succeeded"  # the run recovered despite the rejections
    assert attempts == 1  # the client absorbed the 429s; no task-level retry needed
    assert served == RATE_LIMIT_FAILURES + 1  # it really served N rejections, then a success
    # Gantry's own queue backoff (the fallback layer) is genuinely exponential.
    assert retry_backoff_seconds(2) == 2 * retry_backoff_seconds(1)


async def test_corrupted_sse_stream_is_transparently_retried(db: Sessions, tmp_path: Path) -> None:
    # A malformed mid-stream chunk raises in the client; the run recovers once the
    # mock stops corrupting the stream — whether the client layer retries internally
    # or the failure propagates and the queue re-runs the task, it never fails.
    async with running_server() as base_url:
        status, _, _ = await _drive_until_settled(
            db, base_url, "openai/mock/corrupted-sse", tmp_path, backoff_base=0.0
        )
        served = await _served(base_url, "mock/corrupted-sse")
    assert status == "succeeded"
    # The mock served exactly N corrupted streams, then a clean one — proof the
    # corruption really happened and was recovered, at whichever layer.
    assert served == CORRUPTED_SSE_FAILURES + 1
