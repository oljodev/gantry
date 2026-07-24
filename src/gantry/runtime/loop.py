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

import asyncio
from collections.abc import Awaitable, Callable
from dataclasses import dataclass, replace
from functools import partial
from pathlib import Path
from typing import Any

import sqlalchemy as sa
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.config import get_settings
from gantry.core.db import session_scope
from gantry.core.events import append_event, read_events
from gantry.core.models import TERMINAL_STATUSES, EventType, Project, Task, TaskKind
from gantry.logging import get_logger
from gantry.runtime.compaction import CompactionConfig, CompactionPlan, plan_compaction, summarize
from gantry.runtime.llm import (
    DeltaSink,
    LLMClient,
    LLMResponse,
    LLMUsage,
    Message,
    ToolCallRequest,
)
from gantry.runtime.pricing import cost_usd
from gantry.runtime.state import (
    SPAWN_TOOL_NAMES,
    SURVEY_TOOL_NAMES,
    AgentState,
    TrackedMessage,
    apply_skill_to_system_message,
    assistant_message,
    children_pending_message,
    compaction_anchors,
    leader_land_message,
    leader_nudge_message,
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
    cache_read_tokens: int = 0
    cache_write_tokens: int = 0
    #: Estimated USD spent by this task across all its attempts (priced from the
    #: folded token usage). Recorded on the terminal transition for the run ledger.
    cost_usd: float = 0.0


def _run_cost(state: AgentState, model: str) -> float:
    return cost_usd(
        LLMUsage(
            prompt_tokens=state.prompt_tokens,
            completion_tokens=state.completion_tokens,
            cache_read_tokens=state.cache_read_tokens,
            cache_write_tokens=state.cache_write_tokens,
        ),
        model,
    )


def _outcome(state: AgentState, model: str, final_text: str) -> AgentOutcome:
    return AgentOutcome(
        final_text=final_text,
        steps=state.steps,
        resumed=state.resumed,
        prompt_tokens=state.prompt_tokens,
        completion_tokens=state.completion_tokens,
        cache_read_tokens=state.cache_read_tokens,
        cache_write_tokens=state.cache_write_tokens,
        cost_usd=_run_cost(state, model),
    )


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
    # An autonomous leader has no write tools and exists only to delegate, so the
    # loop budgets how long it may survey before it must start spawning.
    is_leader = bool(payload.get("autonomous_leader"))
    # Optional per-run USD budget (inherited into every task of the run). When
    # this task's own spend crosses it, the loop halts GRACEFULLY at a step
    # boundary (never mid-tool) — the run-level brake that stops a leader fanning
    # out more work lives in the spawn tools.
    budget_usd = payload.get("budget_usd")
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
        # Graceful budget halt: at a step boundary (any in-flight tool has already
        # settled and flushed its git/file state), if this task's own spend crossed
        # the run budget, finish CLEANLY — never a mid-tool kill or a retryable FAIL.
        if budget_usd is not None and _run_cost(state, model) >= float(budget_usd):
            logger.info(
                "agent.budget_halt",
                task_id=str(task.id),
                cost_usd=round(_run_cost(state, model), 4),
            )
            return _outcome(
                state,
                model,
                "Halted: the run's cost budget is exhausted. Stopping cleanly without "
                "starting more work.",
            )

        if is_leader:
            nudge = await _survey_budget_guard(sessions, task, state)
            if nudge is not None:
                state.tracked.append(nudge)

        if compaction is not None:
            await _maybe_compact(sessions, task, state, llm, model, compaction)

        await _checkpoint(
            sessions,
            task,
            EventType.LLM_REQUEST,
            {"step": state.steps + 1, "model": model, "message_count": len(state.tracked)},
        )
        on_delta, flush_reasoning = _reasoning_streamer(sessions, task)
        # Send the PROJECTED history: resolved write/edit bodies elided so a
        # write-heavy worker doesn't recirculate multi-MB args every step (the
        # bloat + per-step-compaction/cache-thrash linchpin). Pure projection —
        # tracked/events keep full args for recover()/replay.
        response = await llm.complete(
            model=model,
            messages=state.projected_messages(),
            tools=tools.schemas(),
            on_delta=on_delta,
        )
        await flush_reasoning()
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
        state.cache_read_tokens += response.usage.cache_read_tokens
        state.cache_write_tokens += response.usage.cache_write_tokens

        if not response.tool_calls:
            reminder = await _children_guard(sessions, task, state)
            if reminder is None and is_leader:
                reminder = await _landing_guard(sessions, task, state)
            if reminder is not None:
                state.tracked.append(reminder)
                continue
            return _outcome(state, model, response.content or "")

        await _settle_tool_calls(
            sessions, task, state, tools, ctx, approval_policy, response.tool_calls
        )


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


def _is_parallel_safe(
    tools: ToolRegistry, policy: ApprovalPolicy | None, tc: ToolCallRequest
) -> bool:
    """A call may join a concurrent batch only if its tool is read-only
    (``parallel_safe``) AND the approval policy would not gate it. Gated calls
    must stay on the sequential path so they can park for human approval, and
    mutating tools must stay sequential so they never race the workspace."""
    tool = tools.get(tc.name)
    if tool is None or not tool.parallel_safe:
        return False
    return policy is None or policy.evaluate(tc.name, tc.arguments) is None


async def _settle_tool_calls(
    sessions: Sessions,
    task: Task,
    state: AgentState,
    tools: ToolRegistry,
    ctx: ToolContext,
    policy: ApprovalPolicy | None,
    tool_calls: tuple[ToolCallRequest, ...],
) -> None:
    """Execute an assistant turn's tool calls, running consecutive runs of
    read-only, ungated calls concurrently while keeping everything else strictly
    sequential and in order. Events are always checkpointed in the original call
    order, so rehydration stays deterministic regardless of completion order."""
    i, n = 0, len(tool_calls)
    while i < n:
        if _is_parallel_safe(tools, policy, tool_calls[i]):
            j = i
            while j < n and _is_parallel_safe(tools, policy, tool_calls[j]):
                j += 1
            await _settle_parallel_batch(sessions, task, state, tools, ctx, tool_calls[i:j])
            i = j
        else:
            await _settle_one_tool_call(sessions, task, state, tools, ctx, policy, tool_calls[i])
            i += 1


async def _settle_one_tool_call(
    sessions: Sessions,
    task: Task,
    state: AgentState,
    tools: ToolRegistry,
    ctx: ToolContext,
    policy: ApprovalPolicy | None,
    tc: ToolCallRequest,
) -> None:
    await _checkpoint(
        sessions,
        task,
        EventType.TOOL_CALL,
        {"tool_call_id": tc.id, "name": tc.name, "arguments": tc.arguments},
    )
    verdict = await _gate_tool_call(sessions, task, state, policy, tc, started=False)
    result = verdict if isinstance(verdict, ToolResult) else await _execute_tool(tools, tc, ctx)
    await _record_tool_result(sessions, task, state, tc, result)


async def _settle_parallel_batch(
    sessions: Sessions,
    task: Task,
    state: AgentState,
    tools: ToolRegistry,
    ctx: ToolContext,
    batch: tuple[ToolCallRequest, ...],
) -> None:
    """Run a run of read-only, ungated calls concurrently. No gating (none of
    these are gated) and no parking (read-only tools never park), so a plain
    gather is safe; each tool already turns its own failure into an error
    result, so gather never raises."""
    for tc in batch:
        await _checkpoint(
            sessions,
            task,
            EventType.TOOL_CALL,
            {"tool_call_id": tc.id, "name": tc.name, "arguments": tc.arguments},
        )
    results = await asyncio.gather(*(_execute_tool(tools, tc, ctx) for tc in batch))
    for tc, result in zip(batch, results, strict=True):
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


#: Backstop against a spawning agent that keeps trying to finish while its
#: children run: after this many nudges the loop lets it finish (a stuck agent
#: shouldn't hang forever). One nudge is almost always enough to route it to
#: wait_for_children, which then parks properly until the whole batch settles.
_MAX_CHILDREN_REMINDERS = 5


async def _children_guard(
    sessions: Sessions, task: Task, state: AgentState
) -> TrackedMessage | None:
    """If the agent tries to finish while children it spawned are still running,
    durably inject a reminder and return it to append — so the loop continues and
    the agent waits instead of orphaning their work. ``None`` means it may finish.
    """
    if state.children_reminders >= _MAX_CHILDREN_REMINDERS:
        return None
    async with session_scope(sessions) as session:
        children = (
            await session.scalars(
                sa.select(Task).where(Task.parent_task_id == task.id).order_by(Task.created_at)
            )
        ).all()
    live = [c for c in children if c.status not in TERMINAL_STATUSES]
    if not live:
        return None
    names = [str(c.payload.get("agent_name") or c.payload.get("goal") or c.id) for c in live]
    seq = await _checkpoint(sessions, task, EventType.CHILDREN_PENDING, {"children": names})
    state.children_reminders += 1
    logger.info("agent.children_guard", task_id=str(task.id), live=len(live))
    return TrackedMessage(seq, children_pending_message(names))


#: How many read-only survey calls a leader may make before the loop starts
#: pushing it to spawn. A handful of reads is plenty to carve up a goal; past
#: this the leader is over-planning rather than delegating.
_LEADER_SURVEY_BUDGET = 12
#: Extra survey calls required between successive nudges, so a leader that keeps
#: reading gets pushed again rather than the nudge firing every single step.
_LEADER_NUDGE_STEP = 3
#: Backstop so a leader that ignores the nudges can't be pestered forever.
_MAX_LEADER_NUDGES = 4


async def _survey_budget_guard(
    sessions: Sessions, task: Task, state: AgentState
) -> TrackedMessage | None:
    """Push an autonomous leader to stop surveying and start spawning.

    A thinking model will deliberate as long as we let it, so the loop caps the
    survey: once the leader has made more than its budget of read-only calls
    without spawning a single worker, durably inject a "delegate now" nudge. The
    nudge stops the instant it spawns anything, and is bounded so it can't loop.
    """
    if state.leader_nudges >= _MAX_LEADER_NUDGES:
        return None
    if state.count_tool_calls(*SPAWN_TOOL_NAMES):
        return None  # already delegating — nothing to nudge
    surveyed = state.count_tool_calls(*SURVEY_TOOL_NAMES)
    if surveyed < _LEADER_SURVEY_BUDGET + state.leader_nudges * _LEADER_NUDGE_STEP:
        return None
    seq = await _checkpoint(sessions, task, EventType.LEADER_NUDGE, {"surveyed": surveyed})
    state.leader_nudges += 1
    logger.info("agent.survey_budget_guard", task_id=str(task.id), surveyed=surveyed)
    return TrackedMessage(seq, leader_nudge_message(surveyed))


#: Backstop so a leader that can't (or won't) land can still finish eventually.
_MAX_LANDING_REMINDERS = 3


async def _landing_guard(
    sessions: Sessions, task: Task, state: AgentState
) -> TrackedMessage | None:
    """If a leader integrated the workers' branches but tries to finish without
    landing them on main, durably remind it to land — so the result reaches the
    default branch instead of sitting on a staging branch. ``None`` lets it
    finish (never merged, already landed, or the reminder cap is reached).
    """
    if state.landing_reminders >= _MAX_LANDING_REMINDERS:
        return None
    if not state.count_tool_calls("merge_child_branches"):
        return None  # nothing was integrated — no staging branch to land
    if state.count_tool_calls("land_branch"):
        return None  # it already tried to land; the tool result guides any retry
    seq = await _checkpoint(sessions, task, EventType.LEADER_NUDGE, {"land": True})
    state.landing_reminders += 1
    logger.info("agent.landing_guard", task_id=str(task.id))
    return TrackedMessage(seq, leader_land_message())


#: Backstop bound on emergency (forced) compactions in a single step, so a
#: pathological over-ceiling state that can't shrink further can't spin.
_MAX_EMERGENCY_COMPACTIONS = 3


async def _apply_compaction(
    sessions: Sessions,
    task: Task,
    state: AgentState,
    llm: LLMClient,
    model: str,
    plan: CompactionPlan,
    projected: list[Message],
) -> None:
    """Summarize the head, checkpoint the COMPACTION event, and fold the live
    history — keeping the [system, goal] anchors verbatim. ``projected`` is the
    elided message list (summarized head + sizing come from it)."""
    result = await summarize(llm, model, state.tracked, plan, projected)
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
    # System (0) and goal (1) are untouchable anchors — kept verbatim so a long
    # run never summarizes away its own instructions/spec. Mirrors the rehydrate
    # fold in state.py exactly (compaction_anchors), or a resume diverges.
    state.tracked = [
        *compaction_anchors(state.tracked),
        TrackedMessage(seq, summary_message(result.summary)),
        *state.tracked[plan.cut_index :],
    ]
    state.prompt_tokens += result.usage.prompt_tokens
    state.completion_tokens += result.usage.completion_tokens
    state.cache_read_tokens += result.usage.cache_read_tokens
    state.cache_write_tokens += result.usage.cache_write_tokens
    logger.info(
        "agent.compacted",
        task_id=str(task.id),
        summarized=result.summarized_messages,
        kept=len(result.kept_seqs),
    )


async def _maybe_compact(
    sessions: Sessions,
    task: Task,
    state: AgentState,
    llm: LLMClient,
    model: str,
    config: CompactionConfig,
) -> None:
    # Size on the PROJECTED (elided) messages — the bytes actually sent — so
    # elision reduces compaction frequency (and thus prefix-cache thrash).
    projected = state.projected_messages()
    plan = plan_compaction(state.tracked, config, projected)
    if plan is not None:
        await _apply_compaction(sessions, task, state, llm, model, plan, projected)
    # Hard per-step ceiling (make a 971K step impossible by invariant): if the
    # projected context still exceeds hard_ceiling, force minimal compactions.
    for _ in range(_MAX_EMERGENCY_COMPACTIONS):
        projected = state.projected_messages()
        if config.token_counter(projected) <= config.hard_ceiling:
            return
        plan = plan_compaction(state.tracked, config, projected, force=True)
        before = len(state.tracked)
        if plan is None:
            break
        await _apply_compaction(sessions, task, state, llm, model, plan, projected)
        if len(state.tracked) >= before:  # couldn't shrink further — stop, don't spin
            break
    if config.token_counter(state.projected_messages()) > config.hard_ceiling:
        logger.warning("agent.compaction_hard_ceiling_exceeded", task_id=str(task.id))


async def _checkpoint(
    sessions: Sessions,
    task: Task,
    event_type: EventType,
    payload: dict[str, Any],
) -> int:
    """One durable checkpoint = one committed transaction."""
    async with session_scope(sessions) as session:
        return await append_event(session, task.id, event_type, payload)


#: Reasoning deltas are coalesced to ~this many chars before a REASONING_CHUNK
#: is emitted — snappy enough to read live without one DB write per token.
_REASONING_FLUSH_CHARS = 120


def _reasoning_streamer(
    sessions: Sessions, task: Task
) -> tuple[DeltaSink, Callable[[], Awaitable[None]]]:
    """A per-step sink that emits a thinking model's reasoning tokens as live
    REASONING_CHUNK events (coalesced), plus a flush for the trailing buffer.
    Content deltas are ignored here — the full content is checkpointed in the
    llm_response — and reasoning is display-only, folded into no agent state."""
    buffer: list[str] = []

    async def flush() -> None:
        if buffer:
            await _checkpoint(sessions, task, EventType.REASONING_CHUNK, {"data": "".join(buffer)})
            buffer.clear()

    async def on_delta(kind: str, text: str) -> None:
        if kind != "reasoning" or not text:
            return
        buffer.append(text)
        if sum(len(part) for part in buffer) >= _REASONING_FLUSH_CHARS:
            await flush()

    return on_delta, flush


def _response_payload(response: LLMResponse) -> dict[str, Any]:
    return {
        "content": response.content,
        "tool_calls": [
            {"id": tc.id, "name": tc.name, "arguments": tc.arguments} for tc in response.tool_calls
        ],
        "model": response.model,
        "finish_reason": response.finish_reason,
        "usage": _usage_payload(response.usage),
        # Recorded for display; state reconstruction never replays it back.
        "reasoning": response.reasoning,
    }


def _usage_payload(usage: LLMUsage) -> dict[str, int]:
    return {
        "prompt_tokens": usage.prompt_tokens,
        "completion_tokens": usage.completion_tokens,
        "cache_read_tokens": usage.cache_read_tokens,
        "cache_write_tokens": usage.cache_write_tokens,
    }
