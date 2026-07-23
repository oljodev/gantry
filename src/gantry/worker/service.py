"""The worker service: claim → sandbox → agent → complete, forever.

Crash-safety posture: the worker never needs a graceful shutdown to be
correct (kill -9 is always safe — Phase 1/2 guarantees). SIGTERM is handled
only as a courtesy: finish the in-flight task, then exit.
"""

from __future__ import annotations

import asyncio
import contextlib
import random
import uuid
from collections.abc import Callable
from dataclasses import dataclass, field
from pathlib import Path

from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.config import Settings
from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.models import Provider, Task, TaskKind, TaskStatus
from gantry.core.notify import QueueListener
from gantry.logging import get_logger
from gantry.runtime.compaction import CompactionConfig
from gantry.runtime.llm import LiteLLMClient, LLMClient
from gantry.runtime.loop import AgentLoopError, run_agent_task
from gantry.runtime.ratelimit import AsyncRateLimiter
from gantry.runtime.tools import TaskParked
from gantry.skills.store import load_registry
from gantry.vault import Vault
from gantry.vault.store import GITHUB_TOKEN_SECRET, get_secret, provider_secret_name
from gantry.worker import workspace as ws
from gantry.worker.git import CloneError
from gantry.worker.policy import policy_for_payload
from gantry.worker.tools import build_coding_registry, build_copilot_registry

logger = get_logger(__name__)

Sessions = async_sessionmaker[AsyncSession]


class LeaseLostError(RuntimeError):
    """Another worker owns this task now; abandon all work on it."""


class TaskCancelledError(RuntimeError):
    """An operator asked to stop this task; abort the run cooperatively."""


class ProviderConfigError(RuntimeError):
    """Task references a provider the worker cannot resolve (non-retryable)."""


#: Builds an LLM client from (api_key, api_base) — the test seam for
#: verifying vault-decrypted credentials reach the client without ever
#: exercising litellm.
LLMFactory = Callable[[str | None, str | None], LLMClient]


@dataclass(frozen=True)
class WorkerConfig:
    worker_id: str
    workspace_root: Path
    lease_seconds: float = 60.0
    poll_interval_seconds: float = 2.0
    github_token: str | None = None
    keep_failed_workspaces: bool = False
    compaction: CompactionConfig | None = field(default_factory=CompactionConfig)
    max_subtasks: int = 32
    skills_root: Path | None = None
    #: Add Anthropic prompt-cache breakpoints to each LLM request (the loop's
    #: append-only history makes the prefix stable, so this is near-free).
    prompt_caching: bool = True
    #: How many agent tasks this process runs at once on the shared event loop.
    #: Default 1 keeps a single-slot worker (the natural unit for tests).
    concurrency: int = 1

    @classmethod
    def from_settings(cls, settings: Settings, worker_id: str | None = None) -> WorkerConfig:
        return cls(
            worker_id=worker_id or f"worker-{uuid.uuid4().hex[:8]}",
            workspace_root=settings.workspace_root,
            github_token=settings.github_token,
            max_subtasks=settings.max_subtasks_per_task,
            skills_root=settings.skills_root,
            prompt_caching=settings.prompt_caching,
            concurrency=settings.worker_concurrency,
            compaction=CompactionConfig(
                max_context_tokens=settings.max_context_tokens,
                keep_recent_messages=settings.keep_recent_messages,
            ),
        )


class Worker:
    def __init__(
        self,
        sessions: Sessions,
        config: WorkerConfig,
        llm: LLMClient,
        listener: QueueListener | None = None,
        *,
        vault: Vault | None = None,
        llm_factory: LLMFactory | None = None,
        limiter: AsyncRateLimiter | None = None,
    ) -> None:
        self._sessions = sessions
        self._config = config
        self._llm = llm
        self._listener = listener
        self._vault = vault
        # Per-provider clients built at claim time share the one process-wide
        # outbound pacer, so the whole fleet throttles as a single stream.
        self._llm_factory: LLMFactory = llm_factory or (
            lambda key, base: LiteLLMClient(
                key, base, prompt_caching=config.prompt_caching, limiter=limiter
            )
        )
        self.processed = 0

    async def run(self, shutdown: asyncio.Event) -> None:
        """Dispatcher loop: claim a task whenever a slot is free and run it as a
        concurrent asyncio task, up to ``concurrency`` at once. One event loop
        drives every agent; a single poller dozes on NOTIFY when the queue is
        empty (no idle-poll storm), and each slot shares the one DB pool and the
        one outbound LLM pacer.
        """
        concurrency = max(1, self._config.concurrency)
        slots = asyncio.Semaphore(concurrency)
        running: set[asyncio.Task[None]] = set()
        logger.info("worker.started", worker_id=self._config.worker_id, concurrency=concurrency)

        async def _run_slot(task: Task) -> None:
            try:
                await self.process(task)
            finally:
                slots.release()

        while not shutdown.is_set():
            if not await self._acquire_slot(slots, shutdown):
                break  # shutdown while waiting for a free slot
            task = await self._claim()
            if task is None:
                slots.release()
                await self._doze(shutdown)
                continue
            slot = asyncio.create_task(_run_slot(task))
            running.add(slot)
            slot.add_done_callback(running.discard)

        # Shutdown: stop claiming and abandon in-flight runs. This is safe by
        # construction — every step is checkpointed, so an abandoned task's lease
        # simply lapses and the reaper re-queues it from its log (kill -9 posture).
        for slot in list(running):
            slot.cancel()
        if running:
            await asyncio.gather(*running, return_exceptions=True)
        logger.info("worker.stopped", worker_id=self._config.worker_id, processed=self.processed)

    async def _claim(self) -> Task | None:
        async with session_scope(self._sessions) as session:
            return await queue.claim(
                session,
                worker_id=self._config.worker_id,
                lease_seconds=self._config.lease_seconds,
            )

    async def _acquire_slot(self, slots: asyncio.Semaphore, shutdown: asyncio.Event) -> bool:
        """Wait for a free slot, staying responsive to shutdown. False if
        shutdown was requested before a slot came free."""
        while not shutdown.is_set():
            try:
                await asyncio.wait_for(slots.acquire(), timeout=0.2)
            except TimeoutError:
                continue
            return True
        return False

    async def _doze(self, shutdown: asyncio.Event) -> None:
        timeout = self._config.poll_interval_seconds * random.uniform(0.8, 1.2)

        async def _wait() -> None:
            if self._listener is not None:
                await self._listener.wait(timeout_seconds=timeout)
            else:
                await asyncio.sleep(timeout)

        stop = asyncio.ensure_future(shutdown.wait())
        waiter = asyncio.ensure_future(_wait())
        try:
            await asyncio.wait({waiter, stop}, return_when=asyncio.FIRST_COMPLETED)
        finally:
            waiter.cancel()
            stop.cancel()
            with contextlib.suppress(asyncio.CancelledError):
                await waiter

    async def process(self, task: Task) -> None:
        cfg = self._config
        logger.info("worker.task_started", task_id=str(task.id), attempt=task.attempt)
        lease_lost = asyncio.Event()
        cancel_requested = asyncio.Event()
        heartbeater = asyncio.create_task(self._heartbeat_loop(task, lease_lost, cancel_requested))
        workspace: ws.Workspace | None = None
        succeeded = False
        try:
            async with session_scope(self._sessions) as session:
                await queue.mark_running(
                    session, task_id=task.id, worker_id=cfg.worker_id, attempt=task.attempt
                )
            copilot = task.payload.get("copilot")
            if copilot:
                # Co-pilot tasks propose a skill/tree for the UI; no sandbox.
                registry = build_copilot_registry(str(copilot))
            else:
                # Every agent gets a sandbox + the coding toolset. A delegating
                # agent (kind=plan, or a profile with can_spawn) additionally gets
                # the orchestration tools, so it can both do hands-on work AND
                # spawn/await children (the coder->reviewer->fix loop).
                workspace = await ws.prepare_workspace(
                    cfg.workspace_root,
                    task.id,
                    task.attempt,
                    task.payload,
                    github_token=await self._github_token(task),
                )
                can_spawn = task.kind is TaskKind.PLAN or bool(task.payload.get("can_spawn"))
                registry = build_coding_registry(
                    workspace.auth,
                    can_spawn=can_spawn,
                    max_subtasks=cfg.max_subtasks,
                    team=task.payload.get("team"),
                )

            async def on_step() -> None:
                # Cancellation wins over lease loss: an operator stop is a
                # deliberate terminal outcome, not something to retry.
                if cancel_requested.is_set():
                    raise TaskCancelledError(str(task.id))
                if lease_lost.is_set():
                    raise LeaseLostError(str(task.id))

            async with self._sessions() as session:
                skills = await load_registry(
                    session, workspace_id=task.workspace_id, project_id=task.project_id
                )
            outcome = await run_agent_task(
                self._sessions,
                task,
                await self._llm_for_task(task),
                registry,
                workspace=workspace.path if workspace else None,
                compaction=cfg.compaction,
                on_step=on_step,
                approval_policy=policy_for_payload(task.payload),
                skills=skills,
            )
            async with session_scope(self._sessions) as session:
                succeeded = await queue.complete(
                    session,
                    task_id=task.id,
                    worker_id=cfg.worker_id,
                    attempt=task.attempt,
                    result={
                        "final_text": outcome.final_text,
                        "steps": outcome.steps,
                        "resumed": outcome.resumed,
                        "branch": workspace.branch if workspace else None,
                        "prompt_tokens": outcome.prompt_tokens,
                        "completion_tokens": outcome.completion_tokens,
                    },
                )
            logger.info("worker.task_succeeded", task_id=str(task.id), steps=outcome.steps)
        except TaskParked as parked:
            async with session_scope(self._sessions) as session:
                if parked.reason == TaskStatus.WAITING_APPROVAL.value:
                    status = await queue.park_for_approval(
                        session, task_id=task.id, worker_id=cfg.worker_id, attempt=task.attempt
                    )
                elif parked.reason == TaskStatus.WAITING_INPUT.value:
                    status = await queue.park_for_input(
                        session, task_id=task.id, worker_id=cfg.worker_id, attempt=task.attempt
                    )
                else:
                    status = await queue.park_for_children(
                        session, task_id=task.id, worker_id=cfg.worker_id, attempt=task.attempt
                    )
            succeeded = True  # the workspace (if any) is not needed while parked
            logger.info(
                "worker.task_parked",
                task_id=str(task.id),
                reason=parked.reason,
                status=str(status),
            )
        except TaskCancelledError:
            async with session_scope(self._sessions) as session:
                await queue.mark_cancelled(
                    session, task_id=task.id, worker_id=cfg.worker_id, attempt=task.attempt
                )
            succeeded = True  # honoured stop — discard the workspace, don't retry
            logger.info("worker.task_cancelled", task_id=str(task.id))
        except LeaseLostError:
            logger.warning("worker.lease_lost", task_id=str(task.id))
        except CloneError as exc:
            # Missing/private/wrong repo: retrying the same clone can't help.
            # Non-retryable with an actionable message (start without a repo).
            await self._fail(task, str(exc), retryable=False)
        except ProviderConfigError as exc:
            # Bad provider reference / vault misconfig: retrying won't help,
            # but retry-after-fixing works because we re-read the row then.
            await self._fail(task, str(exc), retryable=False)
        except AgentLoopError as exc:
            # The agent can't finish (e.g. max steps): retrying won't help.
            await self._fail(task, str(exc), retryable=False)
        except Exception as exc:
            logger.warning("worker.task_errored", task_id=str(task.id), error=repr(exc))
            await self._fail(task, repr(exc), retryable=True)
        finally:
            heartbeater.cancel()
            if workspace is not None and (succeeded or not cfg.keep_failed_workspaces):
                await ws.destroy(workspace.root)
            self.processed += 1

    async def _llm_for_task(self, task: Task) -> LLMClient:
        """The task's LLM client: vault-backed provider config, or the default.

        Tasks without ``provider_id`` use the worker's shared client (env-var
        credentials) — the pre-provider behavior, unchanged.
        """
        raw_provider_id = task.payload.get("provider_id")
        if not raw_provider_id:
            return self._llm
        if self._vault is None:
            raise ProviderConfigError(
                "task uses a configured provider but this worker has no vault key "
                "(set GANTRY_VAULT_KEY)"
            )
        try:
            provider_id = uuid.UUID(str(raw_provider_id))
        except ValueError as exc:
            raise ProviderConfigError(f"invalid provider_id {raw_provider_id!r}") from exc
        async with self._sessions() as session:
            provider = await session.get(Provider, provider_id)
            if provider is None or provider.workspace_id != task.workspace_id:
                raise ProviderConfigError(
                    f"provider {provider_id} not found — was it deleted? "
                    "Recreate it in Settings and retry the task."
                )
            api_key = await get_secret(
                session,
                self._vault,
                workspace_id=task.workspace_id,
                name=provider_secret_name(provider_id),
            )
        return self._llm_factory(api_key, provider.base_url)

    async def _github_token(self, task: Task) -> str | None:
        """Vault-stored GitHub OAuth token, falling back to the env-var token."""
        if self._vault is not None:
            async with self._sessions() as session:
                token = await get_secret(
                    session,
                    self._vault,
                    workspace_id=task.workspace_id,
                    name=GITHUB_TOKEN_SECRET,
                )
            if token:
                logger.info("worker.git_token_source", task_id=str(task.id), source="vault")
                return token
        return self._config.github_token

    async def _fail(self, task: Task, error: str, *, retryable: bool) -> None:
        try:
            async with session_scope(self._sessions) as session:
                status = await queue.fail(
                    session,
                    task_id=task.id,
                    worker_id=self._config.worker_id,
                    attempt=task.attempt,
                    error=error,
                    retryable=retryable,
                )
            logger.info("worker.task_failed", task_id=str(task.id), new_status=status)
        except Exception as exc:  # the reaper will recover the task either way
            logger.warning("worker.fail_report_failed", task_id=str(task.id), error=repr(exc))

    async def _heartbeat_loop(
        self, task: Task, lease_lost: asyncio.Event, cancel_requested: asyncio.Event
    ) -> None:
        # Poll well under the lease so an operator cancel is noticed within a
        # few seconds; each poll also extends the lease (cheap, and it keeps
        # the reaper away), so cancel latency is decoupled from lease length.
        interval = min(5.0, self._config.lease_seconds / 3)
        while True:
            await asyncio.sleep(interval)
            try:
                async with session_scope(self._sessions) as session:
                    beat = await queue.heartbeat(
                        session,
                        task_id=task.id,
                        worker_id=self._config.worker_id,
                        attempt=task.attempt,
                        lease_seconds=self._config.lease_seconds,
                    )
            except Exception as exc:
                logger.warning("worker.heartbeat_error", task_id=str(task.id), error=repr(exc))
                continue
            if not beat.alive:
                lease_lost.set()
                return
            if beat.cancel_requested:
                # Signal the run to abort at its next step, but keep heartbeating
                # so the lease can't lapse (and the reaper re-queue the task)
                # during a long in-flight LLM/tool call before that step.
                cancel_requested.set()
