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
from gantry.core.models import EventType, Project, Task, TaskKind
from gantry.logging import get_logger
from gantry.runtime.compaction import CompactionConfig, plan_compaction, summarize
from gantry.runtime.llm import LLMClient, LLMResponse, LLMUsage, ToolCallRequest
from gantry.runtime.state import (
    AgentState,
    TrackedMessage,
    apply_skill_to_system_message,
    assistant_message,
    rehydrate,
    summary_message,
    tool_message,
)
from gantry.runtime.tools import (
    INTERRUPTED_RESULT,
    ApprovalPolicy,
    TaskParked,
    ToolContext,
    ToolIdempotency,
    ToolRegistry,
    ToolResult,
)
from gantry.skills import SkillRegistry

logger = get_logger(__name__)

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
    approval_policy: ApprovalPolicy | None = None,
    skills: SkillRegistry | None = None,
) -> AgentOutcome:
    payload: dict[str, Any] = task.payload
    settings = get_settings()
    model = payload.get("model") or settings.default_model
    # An orchestrator (a planner, or a profile that can spawn) coordinates and
    # parks while its children work — it legitimately takes many steps, so it is
    # NOT step-capped unless it explicitly asked for one. A plain worker agent
    # uses the default budget.
    can_spawn = task.kind is TaskKind.PLAN or bool(payload.get("can_spawn"))
    explicit_steps = payload.get("max_steps")
    max_steps: int | None
    if explicit_steps:
        max_steps = int(explicit_steps)
    elif can_spawn:
        max_steps = None  # unlimited for delegating agents
    else:
        max_steps = settings.default_max_steps
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

    if skills is not None:
        # Deterministic selection; injection is idempotent per skill name, so
        # a crash between two injections resumes without duplicates. The event
        # pins the exact content this run saw, whatever the file says later.
        for skill in skills.select(payload):
            if skill.name in state.injected_skills:
                continue
            await _checkpoint(
                sessions,
                task,
                EventType.SKILL_INJECTED,
                {"name": skill.name, "description": skill.description, "content": skill.content},
            )
            apply_skill_to_system_message(state, skill.name, skill.content)
            logger.info("agent.skill_injected", task_id=str(task.id), skill=skill.name)

    await _resolve_pending_tool_calls(sessions, task, state, tools, ctx, approval_policy)

    while True:
        if on_step is not None:
            await on_step()
        if max_steps is not None and state.steps >= max_steps:
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
            verdict = await _gate_tool_call(
                sessions, task, state, approval_policy, tc, started=False
            )
            if isinstance(verdict, ToolResult):
                result = verdict
            else:
                result = await _execute_tool(tools, tc, ctx)
            await _record_tool_result(sessions, task, state, tc, result)


async def _auto_approve_enabled(sessions: Sessions, task: Task) -> bool:
    """Whether gated calls should auto-accept: the launch snapshot said so, or
    the project's live toggle is on now. Consulting the live setting makes the
    Approved-page toggle affect runs that were already in flight."""
    if task.payload.get("auto_approve"):
        return True
    async with session_scope(sessions) as session:
        project = await session.get(Project, task.project_id)
        return bool(project is not None and project.auto_approve)


async def _gate_tool_call(
    sessions: Sessions,
    task: Task,
    state: AgentState,
    policy: ApprovalPolicy | None,
    tc: ToolCallRequest,
    *,
    started: bool,
) -> ToolResult | str:
    """Apply the approval gate to a call about to be settled.

    Returns ``"run"`` (execute now), ``"policy"`` (crashed mid-execution —
    apply the tool's idempotency contract), or a ToolResult (human rejection,
    surfaced to the LLM without executing anything). Raises TaskParked when a
    human still has to decide — the dangling call is re-gated on wake.
    """
    decision = policy.evaluate(tc.name, tc.arguments) if policy is not None else None
    if decision is None:
        return "policy" if started else "run"

    approval = state.approvals.get(tc.id)
    if approval is None:
        await _checkpoint(
            sessions,
            task,
            EventType.APPROVAL_REQUESTED,
            {
                "tool_call_id": tc.id,
                "tool": tc.name,
                "arguments": tc.arguments,
                "reason": decision.reason,
                "preview": decision.preview,
            },
        )
        if await _auto_approve_enabled(sessions, task):
            # HITL auto-accept: resolve the gate immediately instead of parking,
            # but still record it (requested + resolved) so the trace is honest.
            await _checkpoint(
                sessions,
                task,
                EventType.APPROVAL_RESOLVED,
                {
                    "tool_call_id": tc.id,
                    "decision": "approved",
                    "comment": "",
                    "resolved_by": "auto-accept",
                },
            )
            await _checkpoint(sessions, task, EventType.TOOL_STARTED, {"tool_call_id": tc.id})
            state.gated_started_ids.add(tc.id)
            logger.info("agent.auto_approved", task_id=str(task.id), tool=tc.name)
            return "run"
        raise TaskParked("waiting_approval", tool_call_id=tc.id)
    if approval.decision == "requested":  # woken for another reason; still undecided
        raise TaskParked("waiting_approval", tool_call_id=tc.id)
    if approval.decision == "rejected":
        comment = approval.comment or "no reason given"
        logger.info("agent.gated_call_rejected", task_id=str(task.id), tool=tc.name)
        return ToolResult(
            f"REJECTED by a human operator: {comment}. Do not retry this exact "
            "action; choose a safer approach or finish with an explanation.",
            is_error=True,
        )
    # Approved. tool_started proves whether execution already began — without
    # it we could not tell "approved but never ran" from "crashed mid-run".
    if tc.id in state.gated_started_ids:
        return "policy"
    await _checkpoint(sessions, task, EventType.TOOL_STARTED, {"tool_call_id": tc.id})
    state.gated_started_ids.add(tc.id)
    logger.info("agent.gated_call_approved", task_id=str(task.id), tool=tc.name)
    return "run"


async def _resolve_pending_tool_calls(
    sessions: Sessions,
    task: Task,
    state: AgentState,
    tools: ToolRegistry,
    ctx: ToolContext,
    approval_policy: ApprovalPolicy | None,
) -> None:
    """Settle tool calls left dangling by a crash or a park."""
    for tc in state.pending_tool_calls():
        started = tc.id in state.started_tool_ids
        if not started:
            await _checkpoint(
                sessions,
                task,
                EventType.TOOL_CALL,
                {"tool_call_id": tc.id, "name": tc.name, "arguments": tc.arguments},
            )
        verdict = await _gate_tool_call(sessions, task, state, approval_policy, tc, started=started)
        if isinstance(verdict, ToolResult):
            result = verdict
        elif verdict == "run":
            result = await _execute_tool(tools, tc, ctx)
        else:  # "policy": execution may have begun — the idempotency contract
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
            "usage": _usage_payload(result.usage),
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
        "usage": _usage_payload(response.usage),
    }


def _usage_payload(usage: LLMUsage) -> dict[str, int]:
    return {
        "prompt_tokens": usage.prompt_tokens,
        "completion_tokens": usage.completion_tokens,
        "cache_read_tokens": usage.cache_read_tokens,
        "cache_write_tokens": usage.cache_write_tokens,
    }
