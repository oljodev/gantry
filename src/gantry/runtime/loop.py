"""The durable agent loop: every step is checkpointed, any crash is resumable.

Protocol per iteration:

1. (maybe) compact context → ``compaction`` event
2. ``llm_request`` event → call the LLM → ``llm_response`` event
3. for each requested tool call: ``tool_call`` event → execute →
   ``tool_result`` event
4. no tool calls requested → the response is the final answer

Each checkpoint is its own transaction. A worker killed between any two
statements loses at most one in-flight step; the next worker rehydrates from
the log (see ``state.rehydrate``) and continues, resolving any dangling tool
call through the tool's idempotency policy.

Unexpected exceptions (LLM/provider failures, DB outages) propagate to the
caller: the task queue's retry-with-backoff plus rehydration make retries
cheap — nothing already checkpointed is redone.
"""

from __future__ import annotations

from collections.abc import Awaitable, Callable
from dataclasses import dataclass, replace
from functools import partial
from pathlib import Path
from typing import Any

from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.config import get_settings
from gantry.core.db import session_scope
from gantry.core.events import append_event, read_events
from gantry.core.models import EventType, Task
from gantry.logging import get_logger
from gantry.runtime.compaction import CompactionConfig, plan_compaction, summarize
from gantry.runtime.llm import LLMClient, LLMResponse, ToolCallRequest
from gantry.runtime.state import (
    AgentState,
    TrackedMessage,
    assistant_message,
    rehydrate,
    summary_message,
    tool_message,
)
from gantry.runtime.tools import (
    INTERRUPTED_RESULT,
    TaskParked,
    ToolContext,
    ToolIdempotency,
    ToolRegistry,
    ToolResult,
)

logger = get_logger(__name__)

DEFAULT_MAX_STEPS = 50

Sessions = async_sessionmaker[AsyncSession]
StepCallback = Callable[[], Awaitable[None]]


class AgentLoopError(RuntimeError):
    """The loop cannot make progress (e.g. max steps exceeded)."""


@dataclass(frozen=True)
class AgentOutcome:
    final_text: str
    steps: int
    resumed: bool
    prompt_tokens: int
    completion_tokens: int


async def run_agent_task(
    sessions: Sessions,
    task: Task,
    llm: LLMClient,
    tools: ToolRegistry,
    *,
    workspace: Path | None = None,
    compaction: CompactionConfig | None = None,
    on_step: StepCallback | None = None,
) -> AgentOutcome:
    payload: dict[str, Any] = task.payload
    model = payload.get("model") or get_settings().default_model
    max_steps = int(payload.get("max_steps") or DEFAULT_MAX_STEPS)
    ctx = ToolContext(
        task_id=task.id,
        workspace=workspace,
        emit_event=partial(_checkpoint, sessions, task),
        sessions=sessions,
    )

    async with session_scope(sessions) as session:
        events = await read_events(session, task.id)
    state = rehydrate(payload, events)
    if state.resumed:
        logger.info(
            "agent.resumed", task_id=str(task.id), steps=state.steps, messages=len(state.tracked)
        )

    await _resolve_pending_tool_calls(sessions, task, state, tools, ctx)

    while True:
        if on_step is not None:
            await on_step()
        if state.steps >= max_steps:
            raise AgentLoopError(f"exceeded max_steps={max_steps} without a final answer")

        if compaction is not None:
            await _maybe_compact(sessions, task, state, llm, model, compaction)

        await _checkpoint(
            sessions,
            task,
            EventType.LLM_REQUEST,
            {"step": state.steps + 1, "model": model, "message_count": len(state.tracked)},
        )
        response = await llm.complete(model=model, messages=state.messages, tools=tools.schemas())
        seq = await _checkpoint(sessions, task, EventType.LLM_RESPONSE, _response_payload(response))
        state.tracked.append(
            TrackedMessage(
                seq,
                assistant_message(
                    response.content,
                    [
                        {"id": tc.id, "name": tc.name, "arguments": tc.arguments}
                        for tc in response.tool_calls
                    ],
                ),
            )
        )
        state.steps += 1
        state.prompt_tokens += response.usage.prompt_tokens
        state.completion_tokens += response.usage.completion_tokens

        if not response.tool_calls:
            return AgentOutcome(
                final_text=response.content or "",
                steps=state.steps,
                resumed=state.resumed,
                prompt_tokens=state.prompt_tokens,
                completion_tokens=state.completion_tokens,
            )

        for tc in response.tool_calls:
            await _checkpoint(
                sessions,
                task,
                EventType.TOOL_CALL,
                {"tool_call_id": tc.id, "name": tc.name, "arguments": tc.arguments},
            )
            result = await _execute_tool(tools, tc, ctx)
            await _record_tool_result(sessions, task, state, tc, result)


async def _resolve_pending_tool_calls(
    sessions: Sessions,
    task: Task,
    state: AgentState,
    tools: ToolRegistry,
    ctx: ToolContext,
) -> None:
    """Settle tool calls left dangling by a crash, per idempotency policy."""
    for tc in state.pending_tool_calls():
        started = tc.id in state.started_tool_ids
        if not started:
            # Never began executing — safe to run regardless of policy.
            await _checkpoint(
                sessions,
                task,
                EventType.TOOL_CALL,
                {"tool_call_id": tc.id, "name": tc.name, "arguments": tc.arguments},
            )
            result = await _execute_tool(tools, tc, ctx)
        else:
            tool = tools.get(tc.name)
            if tool is None:
                result = ToolResult(f"Unknown tool: {tc.name}", is_error=True)
            elif tool.idempotency is ToolIdempotency.IDEMPOTENT:
                logger.info("agent.rerun_interrupted_tool", task_id=str(task.id), tool=tc.name)
                result = await _execute_tool(tools, tc, ctx)
            else:
                recovered = await tool.recover(tc.arguments, replace(ctx, tool_call_id=tc.id))
                result = recovered if recovered is not None else INTERRUPTED_RESULT
                logger.info(
                    "agent.recovered_interrupted_tool",
                    task_id=str(task.id),
                    tool=tc.name,
                    recovered=recovered is not None,
                )
        await _record_tool_result(sessions, task, state, tc, result)


async def _record_tool_result(
    sessions: Sessions,
    task: Task,
    state: AgentState,
    tc: ToolCallRequest,
    result: ToolResult,
) -> None:
    seq = await _checkpoint(
        sessions,
        task,
        EventType.TOOL_RESULT,
        {
            "tool_call_id": tc.id,
            "name": tc.name,
            "content": result.content,
            "is_error": result.is_error,
        },
    )
    state.tracked.append(TrackedMessage(seq, tool_message(tc.id, result.content)))
    state.resolved_tool_ids.add(tc.id)


async def _execute_tool(tools: ToolRegistry, tc: ToolCallRequest, ctx: ToolContext) -> ToolResult:
    tool = tools.get(tc.name)
    if tool is None:
        return ToolResult(f"Unknown tool: {tc.name}", is_error=True)
    try:
        return await tool.execute(tc.arguments, replace(ctx, tool_call_id=tc.id))
    except TaskParked:
        # Control flow, not a tool failure: propagate to the worker, which
        # parks the task. The dangling tool_call checkpoint is the resume
        # point — re-execution on wake produces the real result.
        raise
    except Exception as exc:
        logger.warning("agent.tool_failed", tool=tc.name, error=repr(exc))
        return ToolResult(f"Tool '{tc.name}' failed: {exc!r}", is_error=True)


async def _maybe_compact(
    sessions: Sessions,
    task: Task,
    state: AgentState,
    llm: LLMClient,
    model: str,
    config: CompactionConfig,
) -> None:
    plan = plan_compaction(state.tracked, config)
    if plan is None:
        return
    result = await summarize(llm, model, state.tracked, plan)
    seq = await _checkpoint(
        sessions,
        task,
        EventType.COMPACTION,
        {
            "summary": result.summary,
            "kept_seqs": result.kept_seqs,
            "summarized_messages": result.summarized_messages,
            "usage": {
                "prompt_tokens": result.usage.prompt_tokens,
                "completion_tokens": result.usage.completion_tokens,
            },
        },
    )
    state.tracked = [
        state.tracked[0],
        TrackedMessage(seq, summary_message(result.summary)),
        *state.tracked[plan.cut_index :],
    ]
    state.prompt_tokens += result.usage.prompt_tokens
    state.completion_tokens += result.usage.completion_tokens
    logger.info(
        "agent.compacted",
        task_id=str(task.id),
        summarized=result.summarized_messages,
        kept=len(result.kept_seqs),
    )


async def _checkpoint(
    sessions: Sessions,
    task: Task,
    event_type: EventType,
    payload: dict[str, Any],
) -> int:
    """One durable checkpoint = one committed transaction."""
    async with session_scope(sessions) as session:
        return await append_event(session, task.id, event_type, payload)


def _response_payload(response: LLMResponse) -> dict[str, Any]:
    return {
        "content": response.content,
        "tool_calls": [
            {"id": tc.id, "name": tc.name, "arguments": tc.arguments} for tc in response.tool_calls
        ],
        "model": response.model,
        "finish_reason": response.finish_reason,
        "usage": {
            "prompt_tokens": response.usage.prompt_tokens,
            "completion_tokens": response.usage.completion_tokens,
        },
    }
