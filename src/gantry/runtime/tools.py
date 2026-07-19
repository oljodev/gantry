"""Tool framework: definitions, registry, and crash-recovery policy.

Every tool declares an :class:`ToolIdempotency` policy. It governs what the
loop does when it resumes a task whose log shows a ``tool_call`` checkpoint
with no ``tool_result`` (the worker died somewhere between starting the tool
and recording its outcome):

- ``IDEMPOTENT``     → safe to simply run again (reads, searches, test runs).
- ``NON_IDEMPOTENT`` → the loop calls :meth:`Tool.recover`, which may inspect
  external state (e.g. "did that git push land?") and return the real result;
  if it can't tell, the LLM receives an explicit "interrupted, effect
  unknown" error and decides how to proceed.
"""

from __future__ import annotations

import enum
import uuid
from abc import ABC, abstractmethod
from collections.abc import Awaitable, Callable, Iterable
from dataclasses import dataclass
from pathlib import Path
from typing import Any, ClassVar

from gantry.core.models import EventType
from gantry.runtime.llm import ToolSchema

#: Appends an event to the task's log (bound to the loop's checkpoint writer).
#: Lets tools stream durable side-channel data — e.g. bash terminal chunks.
EventEmitter = Callable[[EventType, dict[str, Any]], Awaitable[int]]


class ToolIdempotency(enum.StrEnum):
    IDEMPOTENT = "idempotent"
    NON_IDEMPOTENT = "non_idempotent"


@dataclass(frozen=True)
class ToolResult:
    content: str
    is_error: bool = False


@dataclass(frozen=True)
class ToolContext:
    task_id: uuid.UUID
    workspace: Path | None = None
    emit_event: EventEmitter | None = None
    #: DB session factory for orchestration tools (spawn/wait). Typed as Any
    #: to keep the runtime layer free of a SQLAlchemy dependency.
    sessions: Any | None = None
    #: The current tool call's id — set per call by the loop. Lets a tool
    #: derive deterministic identifiers (e.g. exactly-once child task ids).
    tool_call_id: str | None = None


class TaskParked(Exception):
    """Raised by a tool to park the task (release compute, keep the log).

    The tool's ``tool_call`` checkpoint stays dangling in the event log; when
    the task is woken and re-claimed, normal crash-recovery re-executes the
    call — which either returns a real result now or parks again. The reason
    becomes the task's parked status (e.g. ``waiting_children``).
    """

    def __init__(self, reason: str) -> None:
        super().__init__(reason)
        self.reason = reason


INTERRUPTED_RESULT = ToolResult(
    content=(
        "This tool call was interrupted by a crash and its effects are unknown. "
        "Verify the current state before assuming it ran or retrying it."
    ),
    is_error=True,
)


class Tool(ABC):
    name: str
    description: str
    #: JSON Schema for the tool's arguments (object schema).
    parameters: ClassVar[dict[str, Any]]
    idempotency: ToolIdempotency = ToolIdempotency.IDEMPOTENT

    @abstractmethod
    async def execute(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult: ...

    async def recover(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult | None:
        """Determine the outcome of a possibly-executed prior call, if possible.

        Only consulted for NON_IDEMPOTENT tools on resume. Return None when
        the outcome can't be determined; the LLM then gets INTERRUPTED_RESULT.
        """
        return None

    def schema(self) -> ToolSchema:
        return {
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": self.parameters,
            },
        }


class ToolRegistry:
    def __init__(self, tools: Iterable[Tool] = ()) -> None:
        self._tools: dict[str, Tool] = {}
        for tool in tools:
            self.register(tool)

    def register(self, tool: Tool) -> None:
        if tool.name in self._tools:
            raise ValueError(f"duplicate tool name: {tool.name}")
        self._tools[tool.name] = tool

    def get(self, name: str) -> Tool | None:
        return self._tools.get(name)

    def schemas(self) -> list[ToolSchema]:
        return [tool.schema() for tool in self._tools.values()]

    def __len__(self) -> int:
        return len(self._tools)
