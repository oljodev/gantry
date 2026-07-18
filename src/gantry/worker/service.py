"""The worker service: claim → sandbox → agent → complete, forever.

Crash-safety posture: the worker never needs a graceful shutdown to be
correct (kill -9 is always safe — Phase 1/2 guarantees). SIGTERM is handled
only as a courtesy: finish the in-flight task, then exit.
"""

from __future__ import annotations

import asyncio
import random
import uuid
from dataclasses import dataclass, field
from pathlib import Path

from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.config import Settings
from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.models import Task
from gantry.core.notify import QueueListener
from gantry.logging import get_logger
from gantry.runtime.compaction import CompactionConfig
from gantry.runtime.llm import LLMClient
from gantry.runtime.loop import AgentLoopError, run_agent_task
from gantry.worker import workspace as ws
from gantry.worker.tools import build_coding_registry

logger = get_logger(__name__)

Sessions = async_sessionmaker[AsyncSession]


class LeaseLostError(RuntimeError):
    """Another worker owns this task now; abandon all work on it."""


@dataclass(frozen=True)
class WorkerConfig:
    worker_id: str
    workspace_root: Path
    lease_seconds: float = 60.0
    poll_interval_seconds: float = 2.0
    github_token: str | None = None
    keep_failed_workspaces: bool = False
    compaction: CompactionConfig | None = field(default_factory=CompactionConfig)

    @classmethod
    def from_settings(cls, settings: Settings, worker_id: str | None = None) -> WorkerConfig:
        return cls(
            worker_id=worker_id or f"worker-{uuid.uuid4().hex[:8]}",
            workspace_root=settings.workspace_root,
            github_token=settings.github_token,
        )


class Worker:
    def __init__(
        self,
        sessions: Sessions,
        config: WorkerConfig,
        llm: LLMClient,
        listener: QueueListener | None = None,
    ) -> None:
        self._sessions = sessions
        self._config = config
        self._llm = llm
        self._listener = listener
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
            workspace = await ws.prepare_workspace(
                cfg.workspace_root,
                task.id,
                task.attempt,
                task.payload,
                github_token=cfg.github_token,
            )
            registry = build_coding_registry(workspace.auth)

            async def on_step() -> None:
                if lease_lost.is_set():
                    raise LeaseLostError(str(task.id))

            outcome = await run_agent_task(
                self._sessions,
                task,
                self._llm,
                registry,
                workspace=workspace.path,
                compaction=cfg.compaction,
                on_step=on_step,
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
                        "branch": workspace.branch,
                        "prompt_tokens": outcome.prompt_tokens,
                        "completion_tokens": outcome.completion_tokens,
                    },
                )
            logger.info("worker.task_succeeded", task_id=str(task.id), steps=outcome.steps)
        except LeaseLostError:
            logger.warning("worker.lease_lost", task_id=str(task.id))
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
