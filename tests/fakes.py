"""Deterministic LLM/tool fakes for runtime tests.

``CountingToolLLM`` is stateless with respect to the process: its next move is
derived purely from the message history (count of tool messages), so a fresh
process resuming from the rehydrated log continues exactly where the dead one
stopped — which is precisely what the kill -9 acceptance test exercises.
"""

from __future__ import annotations

import asyncio
from collections.abc import Sequence
from pathlib import Path
from typing import Any, ClassVar

from gantry.runtime.llm import (
    LLMResponse,
    LLMUsage,
    Message,
    ToolCallRequest,
    ToolSchema,
)
from gantry.runtime.tools import Tool, ToolContext, ToolIdempotency, ToolResult


class ScriptedLLM:
    """Returns canned responses in order; records every call it receives."""

    def __init__(self, responses: list[LLMResponse]) -> None:
        self._responses = list(responses)
        self.calls: list[dict[str, Any]] = []

    async def complete(
        self,
        *,
        model: str,
        messages: list[Message],
        tools: Sequence[ToolSchema] = (),
    ) -> LLMResponse:
        self.calls.append({"model": model, "messages": list(messages), "tools": list(tools)})
        if not self._responses:
            raise AssertionError("ScriptedLLM ran out of responses")
        return self._responses.pop(0)


class CountingToolLLM:
    """Calls `increment` until `target` tool results exist, then answers."""

    def __init__(self, target: int) -> None:
        self.target = target

    async def complete(
        self,
        *,
        model: str,
        messages: list[Message],
        tools: Sequence[ToolSchema] = (),
    ) -> LLMResponse:
        done = sum(1 for m in messages if m.get("role") == "tool")
        if done < self.target:
            return LLMResponse(
                content=None,
                tool_calls=(
                    ToolCallRequest(
                        id=f"call_{done + 1}", name="increment", arguments={"n": done + 1}
                    ),
                ),
                usage=LLMUsage(prompt_tokens=10, completion_tokens=5),
            )
        return LLMResponse(
            content=f"done after {self.target} increments",
            usage=LLMUsage(prompt_tokens=10, completion_tokens=5),
        )


def response_with_tool_call(call_id: str, name: str, arguments: dict[str, Any]) -> LLMResponse:
    return LLMResponse(
        content=None,
        tool_calls=(ToolCallRequest(id=call_id, name=name, arguments=arguments),),
        usage=LLMUsage(prompt_tokens=10, completion_tokens=5),
    )


def final_response(text: str) -> LLMResponse:
    return LLMResponse(content=text, usage=LLMUsage(prompt_tokens=10, completion_tokens=5))


def multi_tool_response(*calls: tuple[str, str, dict[str, Any]]) -> LLMResponse:
    return LLMResponse(
        content=None,
        tool_calls=tuple(
            ToolCallRequest(id=call_id, name=name, arguments=args) for call_id, name, args in calls
        ),
        usage=LLMUsage(prompt_tokens=10, completion_tokens=5),
    )


class OrchestratorLLM:
    """Process-stateless planner + children, routed by the task's goal.

    Any worker in the fleet can hold this one LLM and correctly serve both the
    planner and every child, across parks, wakes, and crashes — every decision
    is derived purely from the message history:

    - goal starts with ``PLAN:``  → planner script:
        1. no tool results yet → spawn one subtask per entry in ``subtask_goals``
        2. spawns acknowledged but no report yet → call ``wait_for_children``
        3. report received → final answer joining the children's final_texts
    - anything else → a child: immediately answer ``answer[<goal>]``.
    """

    def __init__(self, subtask_goals: list[str], child_delay_seconds: float = 0.0) -> None:
        self.subtask_goals = subtask_goals
        self._child_delay = child_delay_seconds

    async def complete(
        self,
        *,
        model: str,
        messages: list[Message],
        tools: Sequence[ToolSchema] = (),
    ) -> LLMResponse:
        goal = str(messages[1].get("content") or "")
        if not goal.startswith("PLAN:"):
            if self._child_delay:
                await asyncio.sleep(self._child_delay)
            return final_response(f"answer[{goal}]")

        tool_contents = [str(m.get("content") or "") for m in messages if m.get("role") == "tool"]
        report = next((c for c in tool_contents if '"children"' in c), None)
        if report is not None:
            import json

            children = json.loads(report)["children"]
            joined = " | ".join(c.get("final_text", c["status"]) for c in children)
            return final_response(f"INTEGRATED: {joined}")
        if len(tool_contents) >= len(self.subtask_goals):
            return response_with_tool_call("wait_1", "wait_for_children", {})
        return multi_tool_response(
            *(
                (f"spawn_{i}", "spawn_subtask", {"goal": g})
                for i, g in enumerate(self.subtask_goals)
            )
        )


class RecordingTool(Tool):
    """In-memory tool that logs every execution; configurable idempotency."""

    description = "Test tool that records calls"
    parameters: ClassVar[dict[str, Any]] = {"type": "object", "properties": {}}

    def __init__(
        self,
        name: str = "increment",
        idempotency: ToolIdempotency = ToolIdempotency.IDEMPOTENT,
        fail_with: Exception | None = None,
        recover_result: ToolResult | None = None,
    ) -> None:
        self.name = name
        self.idempotency = idempotency
        self.executions: list[dict[str, Any]] = []
        self.recover_calls: list[dict[str, Any]] = []
        self._fail_with = fail_with
        self._recover_result = recover_result

    async def execute(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult:
        self.executions.append(arguments)
        if self._fail_with is not None:
            raise self._fail_with
        return ToolResult(content=f"ok:{arguments}")

    async def recover(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult | None:
        self.recover_calls.append(arguments)
        return self._recover_result


class FileIncrementTool(Tool):
    """Appends a line to a file per execution — a cross-process side-effect log.

    The sleep is the deliberate kill window for the crash-recovery test.
    """

    name = "increment"
    description = "Increment the counter"
    parameters: ClassVar[dict[str, Any]] = {
        "type": "object",
        "properties": {"n": {"type": "integer"}},
        "required": ["n"],
    }
    idempotency = ToolIdempotency.IDEMPOTENT

    def __init__(self, effect_file: Path, work_seconds: float = 0.0) -> None:
        self._effect_file = effect_file
        self._work_seconds = work_seconds

    async def execute(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult:
        if self._work_seconds:
            await asyncio.sleep(self._work_seconds)
        with self._effect_file.open("a") as f:
            f.write(f"{arguments.get('n')}\n")
        return ToolResult(content=f"incremented to {arguments.get('n')}")
