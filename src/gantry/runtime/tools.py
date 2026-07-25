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
from typing import Any, ClassVar, Protocol

from gantry.core.models import EventType
from gantry.runtime.diagnostics import Diagnostic
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
    #: Structured problems parsed out of this result (compiler/test/runtime
    #: diagnostics). Carried alongside ``content`` rather than re-parsed by the
    #: loop: the tool has the RAW output, while ``content`` has already been
    #: pruned for the prompt, so parsing downstream would work from the lossy
    #: copy. Empty for tools that produce no diagnostics.
    diagnostics: tuple[Diagnostic, ...] = ()


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
    """Raised to park the task (release compute, keep the log).

    The tool's ``tool_call`` checkpoint stays dangling in the event log; when
    the task is woken and re-claimed, normal crash-recovery re-processes the
    call — which either produces a real result now or parks again. The reason
    becomes the task's parked status (``waiting_children``/``waiting_approval``);
    approval parks carry the gated call's id so the queue can pair the park
    with its resolution.
    """

    def __init__(self, reason: str, tool_call_id: str | None = None) -> None:
        super().__init__(reason)
        self.reason = reason
        self.tool_call_id = tool_call_id


class TaskStalled(Exception):
    """Raised to stop a task that is looping and route it for escalation.

    Control flow, not failure: the same shape as :class:`TaskParked`. The loop
    detector has established that one error keeps recurring, so continuing would
    spend another turn on an approach already disproven. The worker records the
    escalation and re-queues the task; the next claim rehydrates with the
    escalated model and the error history in context.
    """

    def __init__(self, *, fingerprint: str, history: list[str], model: str | None) -> None:
        super().__init__(f"repair loop on {fingerprint}")
        self.fingerprint = fingerprint
        self.history = history
        #: Model to escalate to, or None to retry under the same one with the
        #: loop-break context (still worth doing — the context is the fix).
        self.model = model


@dataclass(frozen=True)
class GateDecision:
    """Why a tool call requires human approval, plus an inbox preview."""

    reason: str
    preview: str


class ApprovalPolicy(Protocol):
    def evaluate(self, name: str, arguments: dict[str, Any]) -> GateDecision | None:
        """Return a GateDecision to require approval, or None to allow."""
        ...


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
    #: Whether this tool is safe to run concurrently with sibling calls in the
    #: same assistant turn. True only for read-only, side-effect-free tools (no
    #: workspace mutation, no parking, no gating expectations) — the loop runs a
    #: consecutive run of these together while keeping everything else strictly
    #: sequential, so approval gating and workspace safety are preserved.
    parallel_safe: ClassVar[bool] = False

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
        # Name-sorted so the tool block of the request is byte-identical from
        # one step to the next. The prompt prefix (system + tools) must not
        # shift for provider prompt caching to hit — Anthropic's breakpoints and
        # DeepSeek/OpenAI automatic prefix caching alike key off a stable prefix.
        # Registration order is already deterministic; sorting also makes two
        # registries with the same tools cache-compatible regardless of build order.
        return sorted(
            (tool.schema() for tool in self._tools.values()),
            key=lambda schema: str(schema.get("function", {}).get("name", "")),
        )

    def __len__(self) -> int:
        return len(self._tools)
