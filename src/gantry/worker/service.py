"""The worker service: claim → sandbox → agent → complete, forever.

Crash-safety posture: the worker never needs a graceful shutdown to be
correct (kill -9 is always safe — Phase 1/2 guarantees). SIGTERM is handled
only as a courtesy: finish the in-flight task, then exit.
"""

from __future__ import annotations

import asyncio
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
from gantry.runtime.tools import TaskParked
from gantry.skills import SkillRegistry
from gantry.vault import Vault
from gantry.vault.store import GITHUB_TOKEN_SECRET, get_secret, provider_secret_name
from gantry.worker import workspace as ws
from gantry.worker.policy import policy_for_payload
from gantry.worker.tools import build_coding_registry, build_planner_registry

logger = get_logger(__name__)

Sessions = async_sessionmaker[AsyncSession]


class LeaseLostError(RuntimeError):
    """Another worker owns this task now; abandon all work on it."""


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

    @classmethod
    def from_settings(cls, settings: Settings, worker_id: str | None = None) -> WorkerConfig:
        return cls(
            worker_id=worker_id or f"worker-{uuid.uuid4().hex[:8]}",
            workspace_root=settings.workspace_root,
            github_token=settings.github_token,
            max_subtasks=settings.max_subtasks_per_task,
            skills_root=settings.skills_root,
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
    ) -> None:
        self._sessions = sessions
        self._config = config
        self._llm = llm
        self._listener = listener
        self._vault = vault
        self._llm_factory: LLMFactory = llm_factory or LiteLLMClient
        self._skills = (
            SkillRegistry.load_dir(config.skills_root)
            if config.skills_root is not None
            else SkillRegistry()
        )
        self.processed = 0

    async def run(self, shutdown: asyncio.Event) -> None:
        """Main loop: claim when work exists, doze on NOTIFY/poll otherwise."""
        logger.info("worker.started", worker_id=self._config.worker_id)
        while not shutdown.is_set():
            task = await self._claim()
            if task is not None:
                await self.process(task)
                continue
            await self._doze()
        logger.info("worker.stopped", worker_id=self._config.worker_id)

    async def _claim(self) -> Task | None:
        async with session_scope(self._sessions) as session:
            return await queue.claim(
                session,
                worker_id=self._config.worker_id,
                lease_seconds=self._config.lease_seconds,
            )

    async def _doze(self) -> None:
        timeout = self._config.poll_interval_seconds * random.uniform(0.8, 1.2)
        if self._listener is not None:
            await self._listener.wait(timeout_seconds=timeout)
        else:
            await asyncio.sleep(timeout)

    async def process(self, task: Task) -> None:
        cfg = self._config
        logger.info("worker.task_started", task_id=str(task.id), attempt=task.attempt)
        lease_lost = asyncio.Event()
        heartbeater = asyncio.create_task(self._heartbeat_loop(task, lease_lost))
        workspace: ws.Workspace | None = None
        succeeded = False
        try:
            async with session_scope(self._sessions) as session:
                await queue.mark_running(
                    session, task_id=task.id, worker_id=cfg.worker_id, attempt=task.attempt
                )
            if task.kind is TaskKind.PLAN:
                # Planners coordinate; they get orchestration tools and no
                # sandbox checkout of their own.
                registry = build_planner_registry(cfg.max_subtasks, team=task.payload.get("team"))
            else:
                workspace = await ws.prepare_workspace(
                    cfg.workspace_root,
                    task.id,
                    task.attempt,
                    task.payload,
                    github_token=await self._github_token(task),
                )
                registry = build_coding_registry(workspace.auth)

            async def on_step() -> None:
                if lease_lost.is_set():
                    raise LeaseLostError(str(task.id))

            outcome = await run_agent_task(
                self._sessions,
                task,
                await self._llm_for_task(task),
                registry,
                workspace=workspace.path if workspace else None,
                compaction=cfg.compaction,
                on_step=on_step,
                approval_policy=policy_for_payload(task.payload),
                skills=self._skills,
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
        except LeaseLostError:
            logger.warning("worker.lease_lost", task_id=str(task.id))
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

    async def _heartbeat_loop(self, task: Task, lease_lost: asyncio.Event) -> None:
        interval = self._config.lease_seconds / 3
        while True:
            await asyncio.sleep(interval)
            try:
                async with session_scope(self._sessions) as session:
                    alive = await queue.heartbeat(
                        session,
                        task_id=task.id,
                        worker_id=self._config.worker_id,
                        attempt=task.attempt,
                        lease_seconds=self._config.lease_seconds,
                    )
            except Exception as exc:
                logger.warning("worker.heartbeat_error", task_id=str(task.id), error=repr(exc))
                continue
            if not alive:
                lease_lost.set()
                return
