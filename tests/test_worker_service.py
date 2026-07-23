"""End-to-end worker tests: claim → sandbox → agent → git delivery → complete.

Runs the real Worker service with a scripted LLM against a real Postgres and
a local bare git remote — the full Phase 3 path minus the LLM provider.
"""

from __future__ import annotations

import asyncio
import os
import uuid
from pathlib import Path
from typing import Any

import sqlalchemy as sa
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.models import (
    DEFAULT_WORKSPACE_ID,
    Provider,
    ProviderType,
    Task,
    TaskEvent,
    TaskKind,
    TaskStatus,
)
from gantry.runtime.llm import LLMClient
from gantry.vault import Vault
from gantry.vault.store import GITHUB_TOKEN_SECRET, provider_secret_name, put_secret
from gantry.worker.service import Worker, WorkerConfig

from .fakes import CountingToolLLM, ScriptedLLM, final_response, response_with_tool_call
from .test_worker_git import git, origin  # noqa: F401  (fixture re-export)

Sessions = async_sessionmaker[AsyncSession]


def test_from_settings_threads_the_compaction_thresholds() -> None:
    from gantry.config import Settings

    settings = Settings(max_context_tokens=33_000, keep_recent_messages=4)
    cfg = WorkerConfig.from_settings(settings)
    assert cfg.compaction is not None
    assert cfg.compaction.max_context_tokens == 33_000
    assert cfg.compaction.keep_recent_messages == 4


def make_worker(db: Sessions, tmp_path: Path, llm: LLMClient) -> Worker:
    config = WorkerConfig(
        worker_id="svc-worker-1",
        workspace_root=tmp_path / "workspaces",
        lease_seconds=30,
        poll_interval_seconds=0.05,
    )
    return Worker(db, config, llm)


async def enqueue(db: Sessions, payload: dict[str, Any]) -> Task:
    async with session_scope(db) as session:
        return await queue.enqueue(
            session,
            workspace_id=DEFAULT_WORKSPACE_ID,
            kind=TaskKind.EXECUTE,
            payload={"model": "fake/test", **payload},
        )


async def get_task(db: Sessions, task: Task) -> Task:
    async with session_scope(db) as session:
        refreshed = await session.get(Task, task.id)
        assert refreshed is not None
        return refreshed


async def test_worker_delivers_a_coding_task_end_to_end(
    db: Sessions,
    origin: Path,  # noqa: F811
    tmp_path: Path,
) -> None:
    task = await enqueue(
        db, {"goal": "add a greeting file and deliver it", "repo_url": str(origin)}
    )
    llm = ScriptedLLM(
        [
            response_with_tool_call(
                "c1", "write_file", {"path": "hello.txt", "content": "hello from gantry\n"}
            ),
            response_with_tool_call("c2", "bash", {"command": "cat hello.txt"}),
            response_with_tool_call("c3", "git_commit_push", {"message": "add greeting"}),
            final_response("delivered the greeting file"),
        ]
    )
    worker = make_worker(db, tmp_path, llm)
    shutdown = asyncio.Event()
    run = asyncio.create_task(worker.run(shutdown))
    try:
        deadline = asyncio.get_running_loop().time() + 30
        while (await get_task(db, task)).status is not TaskStatus.SUCCEEDED:
            assert asyncio.get_running_loop().time() < deadline, "task never succeeded"
            await asyncio.sleep(0.1)
    finally:
        shutdown.set()
        await run

    final = await get_task(db, task)
    assert final.result is not None
    branch = final.result["branch"]
    assert branch.startswith("gantry/task-")
    assert final.result["final_text"] == "delivered the greeting file"

    # Work actually landed on the remote.
    assert git("--git-dir", str(origin), "show", f"{branch}:hello.txt") == "hello from gantry"

    # The bash run was durably streamed as terminal chunks.
    async with session_scope(db) as session:
        chunk_data = (
            (
                await session.execute(
                    sa.text(
                        "SELECT payload->>'data' FROM task_events "
                        "WHERE event_type = 'terminal_chunk'"
                    )
                )
            )
            .scalars()
            .all()
        )
    assert any("hello from gantry" in c for c in chunk_data)

    # Workspace was cleaned up after success.
    assert not any((tmp_path / "workspaces").iterdir())


async def test_worker_failure_requeues_task_for_retry(db: Sessions, tmp_path: Path) -> None:
    task = await enqueue(db, {"goal": "explode"})

    class ExplodingLLM:
        async def complete(self, **kwargs: object) -> object:
            raise RuntimeError("provider melted")

    worker = make_worker(db, tmp_path, ExplodingLLM())  # type: ignore[arg-type]
    claimed = await claim_as(db, worker)
    await worker.process(claimed)

    refreshed = await get_task(db, task)
    assert refreshed.status is TaskStatus.PENDING  # retryable → back in the queue
    assert refreshed.last_error is not None and "provider melted" in refreshed.last_error


async def test_agent_loop_error_fails_terminally(db: Sessions, tmp_path: Path) -> None:
    task = await enqueue(db, {"goal": "loop forever", "max_steps": 1})
    worker = make_worker(db, tmp_path, CountingToolLLM(100))
    # The fake LLM calls `increment`, which exists in RecordingTool tests but
    # not in the coding registry — the loop still burns a step per attempt,
    # then hits max_steps.
    claimed = await claim_as(db, worker)
    await worker.process(claimed)

    refreshed = await get_task(db, task)
    assert refreshed.status is TaskStatus.FAILED  # non-retryable: retrying won't help
    assert refreshed.last_error is not None and "max_steps" in refreshed.last_error


async def claim_as(db: Sessions, worker: Worker) -> Task:
    async with session_scope(db) as session:
        task = await queue.claim(session, worker_id="svc-worker-1", lease_seconds=30)
    assert task is not None
    return task


async def test_worker_run_loop_idles_then_picks_up_new_work(db: Sessions, tmp_path: Path) -> None:
    worker = make_worker(db, tmp_path, ScriptedLLM([final_response("quick win")]))
    shutdown = asyncio.Event()
    run = asyncio.create_task(worker.run(shutdown))
    try:
        await asyncio.sleep(0.2)  # worker is idling on an empty queue
        task = await enqueue(db, {"goal": "trivial"})
        deadline = asyncio.get_running_loop().time() + 15
        while (await get_task(db, task)).status is not TaskStatus.SUCCEEDED:
            assert asyncio.get_running_loop().time() < deadline
            await asyncio.sleep(0.05)
    finally:
        shutdown.set()
        await run
    assert worker.processed == 1


# --- Vault-backed providers ----------------------------------------------


async def seed_provider(db: Sessions, vault: Vault, api_key: str = "sk-vaulted-key-9x7z") -> str:
    async with session_scope(db) as session:
        provider = Provider(
            workspace_id=DEFAULT_WORKSPACE_ID,
            name="vault-provider",
            provider_type=ProviderType.OPENROUTER,
            base_url="https://openrouter.example/api",
            default_model="some/model",
            api_key_last4=api_key[-4:],
        )
        session.add(provider)
        await session.flush()
        await put_secret(
            session,
            vault,
            workspace_id=DEFAULT_WORKSPACE_ID,
            name=provider_secret_name(provider.id),
            plaintext=api_key,
        )
        return str(provider.id)


async def test_provider_credentials_reach_llm_factory(db: Sessions, tmp_path: Path) -> None:
    vault = Vault(os.urandom(32))
    provider_id = await seed_provider(db, vault)
    factory_calls: list[tuple[str | None, str | None]] = []
    scripted = ScriptedLLM([final_response("done via provider")])

    def factory(api_key: str | None, api_base: str | None) -> LLMClient:
        factory_calls.append((api_key, api_base))
        return scripted

    config = WorkerConfig(
        worker_id="svc-worker-1", workspace_root=tmp_path / "ws", poll_interval_seconds=0.05
    )
    worker = Worker(db, config, ScriptedLLM([]), vault=vault, llm_factory=factory)
    task = await enqueue(db, {"goal": "use my provider", "provider_id": provider_id})
    await worker.process(await claim_as(db, worker))

    assert (await get_task(db, task)).status is TaskStatus.SUCCEEDED
    assert factory_calls == [("sk-vaulted-key-9x7z", "https://openrouter.example/api")]

    # The secrecy invariant: the plaintext key appears in no event payload.
    async with db() as session:
        events = (await session.scalars(sa.select(TaskEvent))).all()
    for event in events:
        assert "sk-vaulted-key-9x7z" not in str(event.payload)


async def test_missing_provider_fails_non_retryable(db: Sessions, tmp_path: Path) -> None:
    vault = Vault(os.urandom(32))
    config = WorkerConfig(
        worker_id="svc-worker-1", workspace_root=tmp_path / "ws", poll_interval_seconds=0.05
    )
    worker = Worker(db, config, ScriptedLLM([]), vault=vault)
    task = await enqueue(db, {"goal": "x", "provider_id": str(uuid.uuid4())})
    await worker.process(await claim_as(db, worker))

    refreshed = await get_task(db, task)
    assert refreshed.status is TaskStatus.FAILED
    assert refreshed.last_error is not None and "not found" in refreshed.last_error


async def test_provider_without_vault_fails_clearly(db: Sessions, tmp_path: Path) -> None:
    config = WorkerConfig(
        worker_id="svc-worker-1", workspace_root=tmp_path / "ws", poll_interval_seconds=0.05
    )
    worker = Worker(db, config, ScriptedLLM([]))  # no vault
    task = await enqueue(db, {"goal": "x", "provider_id": str(uuid.uuid4())})
    await worker.process(await claim_as(db, worker))

    refreshed = await get_task(db, task)
    assert refreshed.status is TaskStatus.FAILED
    assert refreshed.last_error is not None and "GANTRY_VAULT_KEY" in refreshed.last_error


async def test_github_token_prefers_vault_over_config(db: Sessions, tmp_path: Path) -> None:
    vault = Vault(os.urandom(32))
    async with session_scope(db) as session:
        await put_secret(
            session,
            vault,
            workspace_id=DEFAULT_WORKSPACE_ID,
            name=GITHUB_TOKEN_SECRET,
            plaintext="gho_from_vault",
        )
    config = WorkerConfig(
        worker_id="svc-worker-1", workspace_root=tmp_path / "ws", github_token="ghp_from_env"
    )
    task = await enqueue(db, {"goal": "x"})
    with_vault = Worker(db, config, ScriptedLLM([]), vault=vault)
    assert await with_vault._github_token(task) == "gho_from_vault"
    without_vault = Worker(db, config, ScriptedLLM([]))
    assert await without_vault._github_token(task) == "ghp_from_env"


async def test_worker_honours_cooperative_cancel(db: Sessions, tmp_path: Path) -> None:
    """An operator cancel of a live task aborts the run at the next step
    boundary and lands it terminal in CANCELLED — not retried."""

    class SlowLoopingLLM:
        """Never finishes: always asks for another bash call, sleeping long
        enough that a heartbeat poll runs between steps."""

        async def complete(self, **kwargs: object) -> Any:
            await asyncio.sleep(0.3)
            return response_with_tool_call("c", "bash", {"command": "true"})

    task = await enqueue(db, {"goal": "loop forever", "max_steps": 100})
    config = WorkerConfig(
        worker_id="cancel-w",
        workspace_root=tmp_path / "workspaces",
        lease_seconds=0.3,  # → heartbeat/cancel poll every 0.1s
        poll_interval_seconds=0.05,
    )
    worker = Worker(db, config, SlowLoopingLLM())
    async with session_scope(db) as session:
        claimed = await queue.claim(session, worker_id="cancel-w", lease_seconds=0.3)
    assert claimed is not None

    # Operator asks to stop while it runs — cooperative request on a live task.
    async with session_scope(db) as session:
        result = await queue.cancel(session, task_id=task.id)
    assert result is not None and result.requested

    await asyncio.wait_for(worker.process(claimed), timeout=15)

    refreshed = await get_task(db, task)
    assert refreshed.status is TaskStatus.CANCELLED
    assert refreshed.claimed_by is None
    async with session_scope(db) as session:
        types = (
            (
                await session.execute(
                    sa.text("SELECT event_type FROM task_events WHERE task_id = :tid"),
                    {"tid": task.id},
                )
            )
            .scalars()
            .all()
        )
    assert "task_cancelled" in types
