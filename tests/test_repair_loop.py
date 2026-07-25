"""The repair-loop circuit breaker: a stuck agent is stopped, not funded.

Without this, an agent that keeps producing the same compile error has no way to
notice — every attempt looks locally reasonable — and neither does the engine, so
the task grinds to its step cap (or its budget) making the same edit. Here the
engine decides: the same structured error recurring means stop, escalate, and
hand a stronger model the error history instead of another identical turn.
"""

from __future__ import annotations

import contextlib
import uuid
from pathlib import Path
from typing import Any, ClassVar

import sqlalchemy as sa
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.models import EventType, Task, TaskEvent, TaskStatus
from gantry.runtime import diagnostics as d
from gantry.runtime.loop import AgentLoopError, run_agent_task
from gantry.runtime.state import repeated_error
from gantry.runtime.tools import (
    TaskStalled,
    Tool,
    ToolContext,
    ToolIdempotency,
    ToolRegistry,
    ToolResult,
)
from gantry.worker.service import Worker, WorkerConfig

from .fakes import final_response, response_with_tool_call
from .test_agent_loop import enqueue_agent_task

Sessions = async_sessionmaker[AsyncSession]

SAME_ERROR = d.Diagnostic(
    file="src/main.rs", line=4, column=20, message="mismatched types", code="E0308", source="rust"
)
OTHER_ERROR = d.Diagnostic(
    file="src/lib.rs", line=9, column=1, message="cannot find value `y`", source="rust"
)


class _StuckBuildTool(Tool):
    """A build that always fails the same way, however the agent edits."""

    name = "build"
    description = "Build the project"
    parameters: ClassVar[dict[str, Any]] = {"type": "object", "properties": {}}
    idempotency = ToolIdempotency.IDEMPOTENT

    def __init__(self, *errors: d.Diagnostic) -> None:
        self._errors = errors or (SAME_ERROR,)
        self.runs = 0

    async def execute(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult:
        self.runs += 1
        return ToolResult(
            content="exit_code=1\n" + d.render(list(self._errors)),
            is_error=True,
            diagnostics=self._errors,
        )


class _AlternatingBuildTool(_StuckBuildTool):
    """Fixes one error and breaks another, forever — the edit/revert loop."""

    async def execute(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult:
        self.runs += 1
        current = SAME_ERROR if self.runs % 2 else OTHER_ERROR
        return ToolResult(
            content="exit_code=1\n" + current.render(), is_error=True, diagnostics=(current,)
        )


class _RetryingLLM:
    """An agent that responds to every failure by building again — the exact
    behaviour the breaker exists to interrupt."""

    def __init__(self) -> None:
        self.calls = 0

    async def complete(self, **kwargs: Any) -> Any:
        self.calls += 1
        return response_with_tool_call(f"call_{self.calls}", "build", {})


# --- the detector ---------------------------------------------------------


def test_three_consecutive_repeats_trip_the_detector() -> None:
    fingerprint = SAME_ERROR.fingerprint
    assert repeated_error([[fingerprint]]) is None
    assert repeated_error([[fingerprint], [fingerprint]]) is None
    assert repeated_error([[fingerprint]] * 3) == fingerprint


def test_alternating_errors_also_trip_it() -> None:
    """ "Fix A, break B, fix B, break A" is the same stagnation wearing a
    different shape, and would never show three CONSECUTIVE identical results."""
    a, b = SAME_ERROR.fingerprint, OTHER_ERROR.fingerprint

    assert repeated_error([[a], [b], [a], [b], [a]]) == min(a, b)


def test_progress_does_not_trip_it() -> None:
    """Different errors each time is an agent working through a list, not looping."""
    batches = [[f"fingerprint-{i}"] for i in range(8)]
    assert repeated_error(batches) is None


def test_one_result_reporting_an_error_many_times_is_one_attempt() -> None:
    """A single compile echoing the same error across targets must not look like
    three attempts at fixing it."""
    fingerprint = SAME_ERROR.fingerprint
    assert repeated_error([[fingerprint, fingerprint, fingerprint]]) is None


# --- the loop stalls ------------------------------------------------------


async def test_loop_stalls_the_task_on_the_third_identical_failure(db: Sessions) -> None:
    """The headline behaviour: the worker stops on the 3rd repeat and does NOT
    fire another provider request."""
    task = await enqueue_agent_task(db)
    tool = _StuckBuildTool()
    llm = _RetryingLLM()

    try:
        await run_agent_task(db, task, llm, ToolRegistry([tool]))
        raise AssertionError("the repair loop was never interrupted")
    except TaskStalled as stalled:
        assert stalled.fingerprint == SAME_ERROR.fingerprint
        assert any("mismatched types" in line for line in stalled.history)

    assert tool.runs == 3, "the build ran more times than the threshold"
    # The 4th LLM call is the one the breaker saves: three build attempts were
    # requested, and nothing was asked of the model afterwards.
    assert llm.calls == 3


async def test_stalling_records_the_evidence_durably(db: Sessions) -> None:
    """The detector's evidence has to survive a resume — repetition across
    attempts is precisely what an in-memory counter would forget."""
    task = await enqueue_agent_task(db)
    with contextlib.suppress(TaskStalled):
        await run_agent_task(db, task, _RetryingLLM(), ToolRegistry([_StuckBuildTool()]))

    async with session_scope(db) as session:
        rows = (
            await session.scalars(
                sa.select(TaskEvent).where(
                    TaskEvent.task_id == task.id,
                    TaskEvent.event_type == EventType.DIAGNOSTICS.value,
                )
            )
        ).all()
    assert len(rows) == 3
    assert rows[0].payload["fingerprints"] == [SAME_ERROR.fingerprint]
    assert rows[0].payload["diagnostics"][0]["code"] == "E0308"


async def test_an_agent_making_progress_is_never_interrupted(db: Sessions) -> None:
    """The breaker must not fire on healthy work: different errors each build,
    then a clean finish."""

    class _ProgressingTool(_StuckBuildTool):
        async def execute(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult:
            self.runs += 1
            err = d.Diagnostic(file=f"src/f{self.runs}.rs", line=self.runs, column=1, message="e")
            return ToolResult(content="exit_code=1", is_error=True, diagnostics=(err,))

    class _ThenFinishLLM:
        def __init__(self) -> None:
            self.calls = 0

        async def complete(self, **kwargs: Any) -> Any:
            self.calls += 1
            if self.calls <= 5:
                return response_with_tool_call(f"c{self.calls}", "build", {})
            return final_response("fixed them all")

    task = await enqueue_agent_task(db)
    outcome = await run_agent_task(db, task, _ThenFinishLLM(), ToolRegistry([_ProgressingTool()]))

    assert outcome.final_text == "fixed them all"


async def test_warnings_alone_never_stall_a_task(db: Sessions) -> None:
    """A build that warns identically every time while making real progress is
    not stuck — counting warnings would halt healthy work."""
    warning = d.Diagnostic(
        file="src/lib.rs", line=22, column=9, message="unused", severity=d.Severity.WARNING
    )

    class _WarnThenFinishLLM:
        def __init__(self) -> None:
            self.calls = 0

        async def complete(self, **kwargs: Any) -> Any:
            self.calls += 1
            if self.calls <= 5:
                return response_with_tool_call(f"c{self.calls}", "build", {})
            return final_response("done")

    task = await enqueue_agent_task(db)
    outcome = await run_agent_task(
        db, task, _WarnThenFinishLLM(), ToolRegistry([_StuckBuildTool(warning)])
    )

    assert outcome.final_text == "done"


async def test_alternating_failures_stall_too(db: Sessions) -> None:
    task = await enqueue_agent_task(db)
    tool = _AlternatingBuildTool()

    try:
        await run_agent_task(db, task, _RetryingLLM(), ToolRegistry([tool]))
        raise AssertionError("the alternating loop was never interrupted")
    except TaskStalled:
        pass

    assert tool.runs <= 6


# --- escalation -----------------------------------------------------------


async def test_escalation_requeues_the_task_with_the_history(db: Sessions) -> None:
    task = await enqueue_agent_task(db)
    async with session_scope(db) as session:
        claimed = await queue.claim(session, worker_id="w1")
    assert claimed is not None

    async with session_scope(db) as session:
        status = await queue.escalate(
            session,
            task_id=task.id,
            worker_id="w1",
            attempt=claimed.attempt,
            fingerprint=SAME_ERROR.fingerprint,
            history=["src/main.rs:4: error[E0308]: mismatched types"],
            model="deepseek/deepseek-r1",
        )

    assert status is TaskStatus.PENDING
    async with session_scope(db) as session:
        refreshed = await session.get(Task, task.id)
        assert refreshed is not None
        assert refreshed.status is TaskStatus.PENDING
        # The launch payload is the immutable snapshot; the escalation is an
        # EVENT, so the task's record never disagrees with what it was launched
        # as — the payload still names the ORIGINAL model.
        assert refreshed.payload["model"] == "fake/test"


async def test_the_escalated_run_uses_the_stronger_model_and_sees_the_errors(
    db: Sessions,
) -> None:
    """After escalation the task resumes on the escalation model with the error
    history in context — the point of routing to a higher-tier reasoner."""
    task = await enqueue_agent_task(db)
    async with session_scope(db) as session:
        claimed = await queue.claim(session, worker_id="w1")
        assert claimed is not None
        await queue.escalate(
            session,
            task_id=task.id,
            worker_id="w1",
            attempt=claimed.attempt,
            fingerprint=SAME_ERROR.fingerprint,
            history=["src/main.rs:4: error[E0308]: mismatched types"],
            model="deepseek/deepseek-r1",
        )

    seen: dict[str, Any] = {}

    class _CapturingLLM:
        async def complete(self, **kwargs: Any) -> Any:
            seen.update(kwargs)
            return final_response("took a different approach")

    async with session_scope(db) as session:
        refreshed = await session.get(Task, task.id)
    assert refreshed is not None
    outcome = await run_agent_task(db, refreshed, _CapturingLLM(), ToolRegistry([]))

    assert outcome.final_text == "took a different approach"
    assert seen["model"] == "deepseek/deepseek-r1"
    prompt = "\n".join(str(m.get("content") or "") for m in seen["messages"])
    assert "repair loop" in prompt
    assert "mismatched types" in prompt  # the structured history, not raw logs


async def test_a_second_stall_fails_terminally_instead_of_looping(db: Sessions) -> None:
    """One escalation is the limit: a task still stuck under the stronger model
    is a planning problem, and a clear terminal failure is a better signal to the
    leader than a task quietly grinding to its step cap."""
    task = await enqueue_agent_task(db)
    async with session_scope(db) as session:
        claimed = await queue.claim(session, worker_id="w1")
        assert claimed is not None
        await queue.escalate(
            session,
            task_id=task.id,
            worker_id="w1",
            attempt=claimed.attempt,
            fingerprint=SAME_ERROR.fingerprint,
            history=["src/main.rs:4: error[E0308]: mismatched types"],
            model="deepseek/deepseek-r1",
        )
    async with session_scope(db) as session:
        refreshed = await session.get(Task, task.id)
    assert refreshed is not None

    tool = _StuckBuildTool()
    try:
        await run_agent_task(db, refreshed, _RetryingLLM(), ToolRegistry([tool]))
        raise AssertionError("a second stall should fail terminally")
    except AgentLoopError as exc:
        assert "persisted after escalation" in str(exc)
        assert "mismatched types" in str(exc)


# --- end to end through the worker ----------------------------------------


async def test_worker_escalates_a_stuck_task_without_spawning_a_duplicate(
    db: Sessions, tmp_path: Path, monkeypatch: Any
) -> None:
    """The full path: the worker catches the stall, re-queues THIS task for
    escalation (no duplicate agent), and the trace explains why."""
    task = await enqueue_agent_task(db, {"copilot": "tree"})  # copilot => no sandbox

    async def _stall(*args: Any, **kwargs: Any) -> Any:
        raise TaskStalled(
            fingerprint=SAME_ERROR.fingerprint,
            history=["src/main.rs:4: error[E0308]: mismatched types"],
            model="deepseek/deepseek-r1",
        )

    monkeypatch.setattr("gantry.worker.service.run_agent_task", _stall)

    config = WorkerConfig(
        worker_id="stall-w", workspace_root=tmp_path / "ws", lease_seconds=30, concurrency=1
    )
    worker = Worker(db, config, _RetryingLLM())
    async with session_scope(db) as session:
        claimed = await queue.claim(session, worker_id="stall-w")
    assert claimed is not None

    await worker.process(claimed)

    async with session_scope(db) as session:
        refreshed = await session.get(Task, task.id)
        events = (
            await session.scalars(
                sa.select(TaskEvent).where(
                    TaskEvent.task_id == task.id,
                    TaskEvent.event_type == EventType.TASK_ESCALATED.value,
                )
            )
        ).all()
        children = (
            await session.scalars(sa.select(Task).where(Task.parent_task_id == task.id))
        ).all()

    assert refreshed is not None
    assert refreshed.status is TaskStatus.PENDING  # re-queued, not failed
    assert len(events) == 1
    assert events[0].payload["fingerprint"]
    assert children == [], "escalation must reuse the task, never spawn a duplicate"


async def test_escalation_does_not_burn_the_error_retry_budget(db: Sessions) -> None:
    """The re-claim bumps `attempt` (the fencing token), so without headroom a
    stalled task would spend a retry just to be handed to a better model."""
    task = await enqueue_agent_task(db)
    async with session_scope(db) as session:
        await session.execute(sa.update(Task).where(Task.id == task.id).values(max_attempts=1))
        claimed = await queue.claim(session, worker_id="w1")
    assert claimed is not None

    async with session_scope(db) as session:
        await queue.escalate(
            session,
            task_id=task.id,
            worker_id="w1",
            attempt=claimed.attempt,
            fingerprint="abc",
            history=[],
            model=None,
        )
        refreshed = await session.get(Task, task.id)

    assert refreshed is not None
    assert refreshed.max_attempts > refreshed.attempt


async def test_escalation_is_fenced_like_every_other_transition(db: Sessions) -> None:
    """A zombie worker whose task was reaped must not be able to escalate it."""
    task = await enqueue_agent_task(db)
    async with session_scope(db) as session:
        claimed = await queue.claim(session, worker_id="w1")
    assert claimed is not None

    async with session_scope(db) as session:
        stale = await queue.escalate(
            session,
            task_id=task.id,
            worker_id="w1",
            attempt=claimed.attempt + 1,  # not the attempt we hold
            fingerprint="abc",
            history=[],
            model=None,
        )
    assert stale is None


def test_escalated_state_survives_a_resume() -> None:
    """Rehydration must carry the escalation forward, or a re-claim would drop
    the stronger model and re-run the loop that stalled."""
    from gantry.core.models import TaskEvent as Event
    from gantry.runtime.state import rehydrate

    events = [
        Event(
            task_id=uuid.uuid4(),
            seq=1,
            event_type=EventType.TASK_ESCALATED,
            payload={"model": "deepseek/deepseek-r1", "history": ["boom"], "fingerprint": "f"},
        )
    ]
    state = rehydrate({"goal": "x"}, events)

    assert state.escalated_model == "deepseek/deepseek-r1"
    assert state.escalations == 1
    # A fresh evidence window: the new approach is judged on its own, rather
    # than re-firing the breaker on the history that triggered it.
    assert state.diagnostic_batches == []
