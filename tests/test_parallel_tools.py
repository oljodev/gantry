"""Read-only tool calls in one assistant turn run concurrently; everything
else stays strictly sequential so gating and workspace safety are preserved."""

from __future__ import annotations

import asyncio
from typing import Any, ClassVar

import pytest
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core.db import session_scope
from gantry.core.events import read_events
from gantry.core.models import EventType
from gantry.runtime.llm import ToolCallRequest
from gantry.runtime.loop import _is_parallel_safe, run_agent_task
from gantry.runtime.tools import (
    ApprovalPolicy,
    GateDecision,
    Tool,
    ToolContext,
    ToolRegistry,
    ToolResult,
)

from .fakes import ScriptedLLM, final_response, multi_tool_response
from .test_agent_loop import enqueue_agent_task

Sessions = async_sessionmaker[AsyncSession]


class BarrierReadTool(Tool):
    """A read-only tool that blocks on a barrier — it can only complete if its
    sibling calls are running concurrently, so a sequential loop would deadlock."""

    name = "read_file"
    parallel_safe = True
    description = "test read tool"
    parameters: ClassVar[dict[str, Any]] = {"type": "object", "properties": {}}

    def __init__(self, barrier: asyncio.Barrier) -> None:
        self._barrier = barrier
        self.order: list[str] = []

    async def execute(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult:
        self.order.append(str(arguments.get("k")))
        await self._barrier.wait()  # releases only when all siblings arrive
        return ToolResult(content=f"read:{arguments.get('k')}")


class RecordingWriteTool(Tool):
    """A mutating tool (not parallel_safe) — must never join a concurrent batch."""

    name = "write_file"
    description = "test write tool"
    parameters: ClassVar[dict[str, Any]] = {"type": "object", "properties": {}}

    def __init__(self) -> None:
        self.calls: list[str] = []

    async def execute(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult:
        self.calls.append(str(arguments.get("k")))
        return ToolResult(content=f"wrote:{arguments.get('k')}")


class GateAll(ApprovalPolicy):
    def evaluate(self, name: str, arguments: dict[str, Any]) -> GateDecision | None:
        return GateDecision(reason="needs approval", preview=name)


async def _tool_results_in_order(db: Sessions, task_id: Any) -> list[str]:
    async with session_scope(db) as session:
        events = await read_events(session, task_id)
    return [e.payload["tool_call_id"] for e in events if e.event_type is EventType.TOOL_RESULT]


async def test_readonly_calls_run_concurrently(db: Sessions) -> None:
    task = await enqueue_agent_task(db)
    barrier = asyncio.Barrier(3)  # all three must be in-flight at once to proceed
    tool = BarrierReadTool(barrier)
    llm = ScriptedLLM(
        [
            multi_tool_response(
                ("c1", "read_file", {"k": "a"}),
                ("c2", "read_file", {"k": "b"}),
                ("c3", "read_file", {"k": "c"}),
            ),
            final_response("done"),
        ]
    )
    # A sequential loop would deadlock on the barrier; the timeout turns that
    # regression into a fast failure instead of a hang.
    outcome = await asyncio.wait_for(run_agent_task(db, task, llm, ToolRegistry([tool])), timeout=5)
    assert outcome.final_text == "done"
    # Results are recorded in the original call order regardless of finish order.
    assert await _tool_results_in_order(db, task.id) == ["c1", "c2", "c3"]


async def test_mixed_batch_keeps_writes_sequential_and_ordered(db: Sessions) -> None:
    task = await enqueue_agent_task(db)
    # [read, write, read]: the write must not be gathered with the reads, and
    # results must land in call order. Reads use a 1-wide barrier (no blocking).
    reads = BarrierReadTool(asyncio.Barrier(1))
    writes = RecordingWriteTool()
    registry = ToolRegistry([reads, writes])
    llm = ScriptedLLM(
        [
            multi_tool_response(
                ("c1", "read_file", {"k": "a"}),
                ("c2", "write_file", {"k": "b"}),
                ("c3", "read_file", {"k": "c"}),
            ),
            final_response("done"),
        ]
    )
    outcome = await asyncio.wait_for(run_agent_task(db, task, llm, registry), timeout=5)
    assert outcome.final_text == "done"
    assert writes.calls == ["b"]
    assert await _tool_results_in_order(db, task.id) == ["c1", "c2", "c3"]


def test_is_parallel_safe_excludes_gated_and_mutating_tools() -> None:
    registry = ToolRegistry([BarrierReadTool(asyncio.Barrier(1)), RecordingWriteTool()])
    read = _call("read_file")
    write = _call("write_file")
    # read-only + ungated -> batchable
    assert _is_parallel_safe(registry, None, read) is True
    # a mutating tool never batches, even ungated
    assert _is_parallel_safe(registry, None, write) is False
    # a gated read-only tool drops back to the sequential (parkable) path
    assert _is_parallel_safe(registry, GateAll(), read) is False
    # unknown tool -> sequential
    assert _is_parallel_safe(registry, None, _call("nope")) is False


def _call(name: str) -> ToolCallRequest:
    return ToolCallRequest(id="x", name=name, arguments={})


@pytest.mark.parametrize("policy", [None, GateAll()])
def test_is_parallel_safe_signature_is_stable(policy: ApprovalPolicy | None) -> None:
    # Guards the (registry, policy, call) contract the loop relies on.
    registry = ToolRegistry([BarrierReadTool(asyncio.Barrier(1))])
    assert isinstance(_is_parallel_safe(registry, policy, _call("read_file")), bool)
