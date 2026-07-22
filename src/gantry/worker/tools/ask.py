"""ask_user: let an agent pause and ask the human a question.

Same durable park/resume machinery as approval gating. On first execution the
tool records an ``ask_user_question`` event and raises :class:`TaskParked`
(status ``waiting_input``); the worker parks the task at zero compute. When an
operator answers, the queue records ``ask_user_answered`` and re-queues the
task; crash-recovery re-runs this dangling call, which now finds the answer and
returns it as the tool result. Re-running before an answer simply re-parks
(the question is emitted once, keyed on the tool-call id).
"""

from __future__ import annotations

from typing import Any, ClassVar, cast

import sqlalchemy as sa
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core.db import session_scope
from gantry.core.models import EventType, TaskEvent, TaskStatus
from gantry.runtime.tools import TaskParked, Tool, ToolContext, ToolIdempotency, ToolResult

Sessions = async_sessionmaker[AsyncSession]

_MAX_OPTIONS = 3


class AskUserTool(Tool):
    name = "ask_user"
    description = (
        "Ask the human operator a question and wait (at zero compute cost) for their "
        "answer, which is returned to you as the tool result. Use this for genuine "
        "decisions only you cannot make: ambiguous requirements, a choice between "
        "approaches, or approval to proceed. Provide up to three suggested options; "
        "the operator may pick one or type a custom answer."
    )
    parameters: ClassVar[dict[str, Any]] = {
        "type": "object",
        "properties": {
            "question": {"type": "string", "description": "The question to ask the operator."},
            "options": {
                "type": "array",
                "items": {"type": "string"},
                "description": "Up to three suggested answers the operator can pick.",
            },
        },
        "required": ["question"],
    }
    #: Re-running just re-checks for an answer (return it) or re-parks — safe.
    idempotency = ToolIdempotency.IDEMPOTENT

    async def execute(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult:
        question = str(arguments.get("question") or "").strip()
        if not question:
            return ToolResult("ask_user: a non-empty question is required", is_error=True)
        if ctx.tool_call_id is None:
            return ToolResult("ask_user requires a tool call id", is_error=True)
        if ctx.sessions is None:
            return ToolResult("ask_user requires ToolContext.sessions", is_error=True)
        options = [str(o) for o in (arguments.get("options") or [])][:_MAX_OPTIONS]
        sessions = cast("Sessions", ctx.sessions)

        async with session_scope(sessions) as session:
            answer, already_asked = await _question_state(session, ctx.task_id, ctx.tool_call_id)
        if answer is not None:
            return ToolResult(answer)
        if not already_asked and ctx.emit_event is not None:
            await ctx.emit_event(
                EventType.ASK_USER_QUESTION,
                {"tool_call_id": ctx.tool_call_id, "question": question, "options": options},
            )
        raise TaskParked(TaskStatus.WAITING_INPUT.value, tool_call_id=ctx.tool_call_id)


async def _question_state(
    session: AsyncSession, task_id: Any, tool_call_id: str
) -> tuple[str | None, bool]:
    """(answer, already_asked) for this call's question, from the event log."""
    events = (
        await session.scalars(
            sa.select(TaskEvent)
            .where(
                TaskEvent.task_id == task_id,
                TaskEvent.event_type.in_(
                    [EventType.ASK_USER_QUESTION.value, EventType.ASK_USER_ANSWERED.value]
                ),
            )
            .order_by(TaskEvent.seq)
        )
    ).all()
    answer: str | None = None
    already_asked = False
    for event in events:
        if str(event.payload.get("tool_call_id")) != tool_call_id:
            continue
        if EventType(event.event_type) is EventType.ASK_USER_QUESTION:
            already_asked = True
        else:
            answer = str(event.payload.get("answer") or "")
    return answer, already_asked
