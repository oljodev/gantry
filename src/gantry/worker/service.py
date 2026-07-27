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
from dataclasses import dataclass, field, replace
from pathlib import Path

from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.config import Settings
from gantry.core import queue
from gantry.core.control import get_control
from gantry.core.db import session_scope
from gantry.core.models import DEFAULT_WORKSPACE_ID, Provider, Task, TaskKind, TaskStatus
from gantry.core.notify import QueueListener
from gantry.logging import get_logger
from gantry.runtime.compaction import CompactionConfig
from gantry.runtime.llm import LiteLLMClient, LLMClient
from gantry.runtime.loop import AgentLoopError, run_agent_task
from gantry.runtime.ratelimit import AsyncRateLimiter, LimiterRegistry
from gantry.runtime.tools import TaskParked, TaskStalled
from gantry.skills.store import load_registry
from gantry.vault import Vault
from gantry.vault.store import GITHUB_TOKEN_SECRET, get_secret, provider_secret_name
from gantry.worker import workspace as ws
from gantry.worker.git import CloneError, GitError, ensure_pushed
from gantry.worker.merge import default_branch, promote_branch
from gantry.worker.policy import policy_for_payload
from gantry.worker.tools import build_coding_registry, build_copilot_registry
from gantry.worker.tools.integrate import make_conflict_resolver

logger = get_logger(__name__)

Sessions = async_sessionmaker[AsyncSession]


#: HTTP statuses from a provider that a retry can never fix: invalid model /
#: malformed request (400), bad key (401), forbidden (403), not found (404).
#: Checked by attribute so we never import the heavy litellm at module load.
_PERMANENT_PROVIDER_STATUSES = frozenset({400, 401, 403, 404})


def _is_permanent_provider_error(exc: Exception) -> bool:
    status = getattr(exc, "status_code", None)
    return isinstance(status, int) and status in _PERMANENT_PROVIDER_STATUSES


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
    #: Spawn circuit breakers (see config): refuse to spawn once a parent has this
    #: many terminally-FAILED direct children (repair-wave loop), or once the whole
    #: run tree hits this many tasks (structural fan-out backstop).
    max_repair_failures: int = 5
    run_task_ceiling: int = 50
    skills_root: Path | None = None
    #: Add Anthropic prompt-cache breakpoints to each LLM request (the loop's
    #: append-only history makes the prefix stable, so this is near-free).
    prompt_caching: bool = True
    #: Per-request LLM timeout (seconds) — bounds a hung provider stream.
    llm_request_timeout_seconds: float = 600.0
    #: How many agent tasks this process runs at once on the shared event loop.
    #: Default 1 keeps a single-slot worker (the natural unit for tests).
    concurrency: int = 1
    #: Fallback model when a task pins none — mirrors the loop's resolution so the
    #: conflict resolver targets the same model the leader runs.
    default_model: str = "anthropic/claude-opus-4-8"
    #: Optional cheaper model for merge-conflict resolution (None = leader's model).
    conflict_resolver_model: str | None = None
    #: Per-role compaction soft caps (None -> use ``compaction``'s cap for every
    #: role). ``compaction_for(task)`` picks between them from the durable kind/
    #: payload so a resume selects the identical threshold.
    execute_max_context_tokens: int | None = None
    leader_max_context_tokens: int | None = None
    #: How often the dispatcher re-reads the workspace emergency stop from the
    #: database. NOTIFY makes a trip propagate instantly; this is the durable
    #: fallback that bounds how long a worker with a dead listener keeps
    #: claiming, so it stays short.
    control_refresh_seconds: float = 2.0

    @classmethod
    def from_settings(cls, settings: Settings, worker_id: str | None = None) -> WorkerConfig:
        return cls(
            worker_id=worker_id or f"worker-{uuid.uuid4().hex[:8]}",
            workspace_root=settings.workspace_root,
            github_token=settings.github_token,
            max_subtasks=settings.max_subtasks_per_task,
            max_repair_failures=settings.max_repair_failures,
            run_task_ceiling=settings.run_task_ceiling,
            skills_root=settings.skills_root,
            prompt_caching=settings.prompt_caching,
            llm_request_timeout_seconds=settings.llm_request_timeout_seconds,
            concurrency=settings.worker_concurrency,
            default_model=settings.default_model,
            conflict_resolver_model=settings.conflict_resolver_model,
            execute_max_context_tokens=settings.execute_max_context_tokens,
            leader_max_context_tokens=settings.leader_max_context_tokens,
            compaction=CompactionConfig(
                max_context_tokens=settings.max_context_tokens,
                keep_recent_messages=settings.keep_recent_messages,
                keep_recent_tokens=settings.keep_recent_tokens,
                hard_max_context_tokens=settings.hard_max_context_tokens,
            ),
        )

    def compaction_for(self, task: Task) -> CompactionConfig | None:
        """The compaction config for one task, its soft cap chosen by role from the
        durable kind/payload (delegating leader vs leaf EXECUTE worker). A per-task
        ``max_context_tokens`` payload value overrides the role default. The tail
        budget is capped at a third of the soft cap so a tighter role can't re-fire
        compaction every step (see CompactionConfig's keep_recent_tokens note)."""
        base = self.compaction
        if base is None:
            return None
        override = task.payload.get("max_context_tokens")
        if override:
            cap: int | None = int(override)
        else:
            delegates = (
                task.kind is TaskKind.PLAN
                or bool(task.payload.get("can_spawn"))
                or bool(task.payload.get("autonomous_leader"))
            )
            cap = self.leader_max_context_tokens if delegates else self.execute_max_context_tokens
        if not cap or cap == base.max_context_tokens:
            return base
        tail = base.keep_recent_tokens if base.keep_recent_tokens is not None else cap // 3
        return replace(base, max_context_tokens=cap, keep_recent_tokens=min(tail, max(1, cap // 3)))


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
        limiter_registry: LimiterRegistry | None = None,
        cancel_listener: QueueListener | None = None,
        control_listener: QueueListener | None = None,
        workspace_id: uuid.UUID = DEFAULT_WORKSPACE_ID,
    ) -> None:
        self._sessions = sessions
        self._config = config
        self._llm = llm
        self._listener = listener
        self._cancel_listener = cancel_listener
        self._control_listener = control_listener
        self._workspace_id = workspace_id
        self._vault = vault
        #: Cached emergency-stop gate. Refreshed on a timer (durable fallback)
        #: and flipped instantly by the control NOTIFY. ``_control_checked_at``
        #: is loop-clock time, so it never depends on the host wall clock.
        self._stopped = False
        self._stop_reason = ""
        self._control_checked_at = 0.0
        #: Slots currently running an agent, by task id — the target of a hard
        #: cancel. ``_stopping`` marks the ones being deliberately stopped so the
        #: slot records them CANCELLED (vs a shutdown cancel, left for the reaper).
        self._running: dict[uuid.UUID, asyncio.Task[None]] = {}
        self._stopping: set[uuid.UUID] = set()

        # Per-provider clients built at claim time each pace on THEIR provider's
        # limiter (keyed by base_url), so one throttled provider can't stall
        # another; the keyless default client shares the "" limiter. Falls back to
        # a single shared limiter when no registry is supplied (tests).
        def _limiter_for(base: str | None) -> AsyncRateLimiter | None:
            return limiter_registry.get(base) if limiter_registry is not None else limiter

        self._llm_factory: LLMFactory = llm_factory or (
            lambda key, base: LiteLLMClient(
                key,
                base,
                prompt_caching=config.prompt_caching,
                limiter=_limiter_for(base),
                request_timeout=config.llm_request_timeout_seconds,
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
        # An operator "Stop all" NOTIFYs the cancel channel; interrupt the slot now.
        if self._cancel_listener is not None:
            self._cancel_listener.on_payload = self._on_cancel_notify
        # The workspace kill switch: force an immediate re-read on any change.
        if self._control_listener is not None:
            self._control_listener.on_payload = self._on_control_notify
        logger.info("worker.started", worker_id=self._config.worker_id, concurrency=concurrency)

        async def _run_slot(task: Task) -> None:
            try:
                await self.process(task)
            except asyncio.CancelledError:
                # A deliberate operator stop lands the task CANCELLED right now; a
                # shutdown cancel is abandoned for the reaper to re-queue.
                if task.id in self._stopping:
                    await self._finalize_stop(task)
                    return
                raise
            finally:
                self._stopping.discard(task.id)
                self._running.pop(task.id, None)
                slots.release()

        while not shutdown.is_set():
            if not await self._acquire_slot(slots, shutdown):
                break  # shutdown while waiting for a free slot
            # The kill switch is checked BEFORE every claim, so a tripped stop
            # cannot admit even one more task — the whole point is that spend
            # stops immediately, not after the current wave drains.
            if await self._emergency_stopped():
                slots.release()
                await self._halt_running()
                await self._doze(shutdown)
                continue
            task = await self._claim()
            if task is None:
                slots.release()
                await self._doze(shutdown)
                continue
            slot = asyncio.create_task(_run_slot(task))
            self._running[task.id] = slot

        # Shutdown: stop claiming and abandon in-flight runs. This is safe by
        # construction — every step is checkpointed, so an abandoned task's lease
        # simply lapses and the reaper re-queues it from its log (kill -9 posture).
        for slot in list(self._running.values()):
            slot.cancel()
        if self._running:
            await asyncio.gather(*self._running.values(), return_exceptions=True)
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

    def _on_cancel_notify(self, payload: str) -> None:
        """Cancel-channel handler (runs on the event loop): stop the named slot."""
        try:
            task_id = uuid.UUID(payload)
        except ValueError:
            return
        self._request_hard_cancel(task_id)

    def _on_control_notify(self, payload: str) -> None:
        """Control-channel handler: invalidate the cached gate so the next
        dispatch iteration re-reads it from the database immediately."""
        if payload and payload != str(self._workspace_id):
            return
        self._control_checked_at = 0.0

    async def _emergency_stopped(self) -> bool:
        """Whether this workspace's kill switch is tripped.

        Cached for ``control_refresh_seconds`` so a 100-slot dispatcher spinning
        through free slots does not issue a query per iteration; the control
        NOTIFY zeroes the cache, so a real trip is observed immediately rather
        than up to one interval later. A failed read is deliberately treated as
        "not stopped": the queue's own guards (budgets, leases) still bound a
        run, and refusing to work because a status query blipped would turn a
        transient DB hiccup into a fleet-wide outage.
        """
        now = asyncio.get_running_loop().time()
        if now - self._control_checked_at < self._config.control_refresh_seconds:
            return self._stopped
        self._control_checked_at = now
        try:
            async with self._sessions() as session:
                state = await get_control(session, self._workspace_id)
        except Exception as exc:
            logger.warning("worker.control_read_failed", error=repr(exc))
            return self._stopped
        if state.stopped and not self._stopped:
            logger.warning(
                "worker.emergency_stop_engaged",
                worker_id=self._config.worker_id,
                reason=state.reason,
                running=len(self._running),
            )
        elif self._stopped and not state.stopped:
            logger.info("worker.emergency_stop_cleared", worker_id=self._config.worker_id)
        self._stopped, self._stop_reason = state.stopped, state.reason
        return self._stopped

    async def _halt_running(self) -> None:
        """Stop every agent this worker is running, for an emergency stop.

        Uses the same hard-cancel path as an operator "stop this task": each slot
        is interrupted mid-step and lands CANCELLED, so its event log survives
        and the task can be retried once the stop is cleared. Draining instead —
        letting in-flight agents finish — would keep the exact spend the switch
        exists to stop, sometimes for many minutes.
        """
        for task_id in list(self._running):
            self._request_hard_cancel(task_id)

    def _request_hard_cancel(self, task_id: uuid.UUID) -> None:
        """Interrupt a running slot immediately (mid LLM/tool call). A no-op if
        this worker isn't running that task, or is already stopping it.

        The already-stopping guard is load-bearing: a slot that has been
        cancelled is running its cleanup (``_finalize_stop``, which awaits the
        CANCELLED write), and a SECOND ``cancel()`` lands on that await and
        aborts it — so the task never records its terminal state and the reaper
        later re-queues a task an operator explicitly stopped. Every cancel path
        funnels through here (cancel NOTIFY, heartbeat poll, emergency halt), so
        one guard covers them all.
        """
        if task_id in self._stopping:
            return
        slot = self._running.get(task_id)
        if slot is not None and not slot.done():
            self._stopping.add(task_id)
            slot.cancel()
            logger.info("worker.hard_cancel", task_id=str(task_id))

    async def _finalize_stop(self, task: Task) -> None:
        """Record a hard-stopped task as CANCELLED (its cleanup already ran in
        process()'s finally). Idempotent via the lease compare-and-set."""
        try:
            async with session_scope(self._sessions) as session:
                await queue.mark_cancelled(
                    session,
                    task_id=task.id,
                    worker_id=self._config.worker_id,
                    attempt=task.attempt,
                )
            logger.info("worker.task_stopped", task_id=str(task.id))
        except Exception as exc:  # the reaper still recovers it if this fails
            logger.warning("worker.stop_mark_failed", task_id=str(task.id), error=repr(exc))

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
            llm = await self._llm_for_task(task)
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
                is_leader = bool(task.payload.get("autonomous_leader"))
                can_spawn = (
                    task.kind is TaskKind.PLAN or bool(task.payload.get("can_spawn")) or is_leader
                )
                resolver_model = (
                    cfg.conflict_resolver_model or task.payload.get("model") or cfg.default_model
                )
                registry = build_coding_registry(
                    workspace.auth,
                    can_spawn=can_spawn,
                    leader=is_leader,
                    max_subtasks=cfg.max_subtasks,
                    max_repair_failures=cfg.max_repair_failures,
                    run_task_ceiling=cfg.run_task_ceiling,
                    team=task.payload.get("team"),
                    trunk_branch=workspace.branch,
                    base_branch=task.payload.get("base_branch"),
                    conflict_resolver=make_conflict_resolver(llm, str(resolver_model)),
                    staging_verify_command=task.payload.get("staging_verify_command"),
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
                llm,
                registry,
                workspace=workspace.path if workspace else None,
                compaction=cfg.compaction_for(task),
                on_step=on_step,
                approval_policy=policy_for_payload(task.payload),
                skills=skills,
            )
            # Before the task counts as succeeded, guarantee its branch reached
            # origin — the swarm's only rendezvous. A child that finished with work
            # committed locally but never pushed would otherwise look succeeded yet
            # deliver nothing to the leader's merge.
            if workspace is not None:
                await self._deliver_branch(task, workspace)
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
                        "cache_read_tokens": outcome.cache_read_tokens,
                        "cache_write_tokens": outcome.cache_write_tokens,
                        "cost_usd": round(outcome.cost_usd, 6),
                        "compactions": outcome.compactions,
                    },
                    cost_usd=outcome.cost_usd,
                )
            logger.info(
                "worker.task_succeeded",
                task_id=str(task.id),
                steps=outcome.steps,
                compactions=outcome.compactions,
                cache_hit=round(outcome.cache_hit_ratio, 3),
                cost_usd=round(outcome.cost_usd, 4),
            )
            await self._maybe_log_run_rollup(task)
            if succeeded and self._should_land_on_main(task, workspace, is_leader):
                assert workspace is not None
                await self._land_task_on_main(task, workspace)
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
        except TaskStalled as stalled:
            # The loop detector broke a repair loop. Re-queue for escalation
            # rather than fail: the event log survives, so the next claim resumes
            # this exact task on a stronger model with the error history in
            # context — no work is redone and no duplicate agent is spawned.
            async with session_scope(self._sessions) as session:
                status = await queue.escalate(
                    session,
                    task_id=task.id,
                    worker_id=cfg.worker_id,
                    attempt=task.attempt,
                    fingerprint=stalled.fingerprint,
                    history=stalled.history,
                    model=stalled.model,
                )
            succeeded = True  # a clean hand-off, not a failure; drop the workspace
            logger.warning(
                "worker.task_escalated",
                task_id=str(task.id),
                fingerprint=stalled.fingerprint,
                model=stalled.model,
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
            # A client-side provider error (invalid model, bad key, forbidden)
            # can never succeed on retry — fail it fast with a clear message
            # instead of burning every attempt (and flooding the log) on it.
            retryable = not _is_permanent_provider_error(exc)
            logger.warning(
                "worker.task_errored",
                task_id=str(task.id),
                error=repr(exc),
                retryable=retryable,
            )
            await self._fail(task, repr(exc), retryable=retryable)
        finally:
            heartbeater.cancel()
            # A deliberate hard-cancel (slot.cancel -> CancelledError) leaves
            # succeeded=False, but a cancel is an "undo", not a failure to debug:
            # always discard its throwaway workspace so the dirty/uncommitted git
            # state is rolled back cleanly (nothing was pushed, so destroy is the
            # rollback). keep_failed_workspaces only preserves genuine failures.
            hard_cancelled = task.id in self._stopping
            if workspace is not None and (
                succeeded or hard_cancelled or not cfg.keep_failed_workspaces
            ):
                await ws.destroy(workspace.root)
            self.processed += 1

    async def _maybe_log_run_rollup(self, task: Task) -> None:
        """When a run's ROOT task settles, log a one-line rollup for the whole tree
        (spend, cache-hit ratio, compactions, per-status counts) — the signal an
        operator watches to see a run's cost/cache health. Best-effort: a rollup
        query failure never affects the task's own outcome."""
        if task.parent_task_id is not None:
            return  # only the root task rolls up the tree; children are summed into it
        try:
            async with self._sessions() as session:
                rollup = await queue.run_rollup(session, task.root_task_id)
        except Exception as exc:
            logger.warning("worker.run_rollup_failed", task_id=str(task.id), error=repr(exc))
            return
        logger.info("worker.run_rollup", root_task_id=str(task.root_task_id), **rollup)

    @staticmethod
    def _should_land_on_main(task: Task, workspace: ws.Workspace | None, is_leader: bool) -> bool:
        """Whether a succeeded task should auto-land its branch on main.

        Only a top-level, git-backed, non-leader task does: a user launched it and
        wants the result live on main, not on a side branch. Spawned children
        (parent set) feed the leader's staging merge — landing them straight to
        main would bypass integration and QA. A leader's own branch is empty (it
        lands the integrated staging branch via land_branch), so it never lands
        here either.
        """
        return (
            workspace is not None
            and workspace.branch is not None
            and task.parent_task_id is None
            and not is_leader
        )

    async def _deliver_branch(self, task: Task, workspace: ws.Workspace) -> None:
        """Ensure a repo-backed task's branch and its commits reach origin before it
        is marked succeeded.

        The swarm's ONLY rendezvous is origin: a leader integrates its children by
        fetching their pushed branches — it cannot read a child's isolated,
        throwaway workspace. So a child that finished with work committed locally
        but never pushed (it forgot ``git_commit_push``, or a push failed) looks
        succeeded yet contributes nothing to the merge, which then integrates an
        empty tree. This convergently commits any outstanding changes and pushes
        HEAD, so "succeeded" implies "delivered". Best-effort and never fatal: if
        the push is rejected the leader's merge tool reports the missing branch as a
        structured diagnostic rather than assuming success.
        """
        if workspace.branch is None:
            return
        try:
            await ensure_pushed(workspace.path, workspace.auth)
        except GitError as exc:
            logger.warning("worker.deliver_failed", task_id=str(task.id), error=repr(exc))

    async def _land_task_on_main(self, task: Task, workspace: ws.Workspace) -> None:
        """Land a finished top-level task's branch on the repo's main branch.

        Best-effort and never fatal to the run: it pushes without --force, so if
        main moved since the task branched the push is rejected and the work
        simply stays on its branch (logged), rather than clobbering main. The
        agent already committed its work with its own message; this just makes it
        live on the default branch instead of a side branch.
        """
        assert workspace.branch is not None
        target = task.payload.get("base_branch") or await default_branch(
            workspace.path, workspace.auth
        )
        try:
            landed, detail = await promote_branch(
                workspace.path, branch=workspace.branch, target=str(target), auth=workspace.auth
            )
        except Exception as exc:  # never let landing failure fail a succeeded task
            logger.warning("worker.land_errored", task_id=str(task.id), error=repr(exc))
            return
        if landed:
            logger.info("worker.landed_on_main", task_id=str(task.id), target=str(target))
        else:
            logger.info("worker.land_skipped", task_id=str(task.id), detail=detail)

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
            if status is TaskStatus.FAILED:  # terminal (not re-queued) — roll a root up
                await self._maybe_log_run_rollup(task)
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
                # Fast path: interrupt the in-flight step now if this task runs
                # under the dispatcher (a no-op otherwise). Fallback: set the
                # cooperative flag so a directly-driven process() still aborts at
                # its next step. Keep heartbeating either way so the lease holds.
                self._request_hard_cancel(task.id)
                cancel_requested.set()
