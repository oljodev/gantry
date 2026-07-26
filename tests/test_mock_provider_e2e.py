"""End-to-end: drive the REAL agent loop, REAL tools, and REAL circuit breakers
against the live mock provider over a real socket — proving the breakers fire for
$0 of token spend. DB-backed (the `db` fixture), but no full WorkerService.
"""

from __future__ import annotations

from pathlib import Path

import pytest
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.runtime.llm import LiteLLMClient
from gantry.runtime.loop import AgentLoopError, run_agent_task
from gantry.runtime.tools import TaskStalled
from gantry.worker.tools import build_coding_registry

from .mock_provider.scenarios import HAPPY_WRITE_BODY, HAPPY_WRITE_PATH
from .mock_provider.server import running_server
from .test_agent_loop import enqueue_agent_task

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
