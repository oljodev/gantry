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

from gantry.config import Settings, get_settings
from gantry.core.db import session_scope
from gantry.core.events import append_event, read_events
from gantry.core.models import TERMINAL_STATUSES, EventType, Project, Task, TaskKind
from gantry.logging import get_logger
from gantry.runtime import diagnostics
from gantry.runtime.compaction import CompactionConfig, CompactionPlan, plan_compaction, summarize
from gantry.runtime.llm import (
    DeltaSink,
    LLMClient,
    LLMResponse,
    LLMUsage,
    Message,
    ToolCallRequest,
    malformed_arguments,
)
from gantry.runtime.pricing import cost_usd
from gantry.runtime.state import (
    PRODUCTIVE_TOOL_NAMES,
    READ_ONLY_TOOL_NAMES,
    SPAWN_TOOL_NAMES,
    SURVEY_TOOL_NAMES,
    AgentState,
    TrackedMessage,
    apply_skill_to_system_message,
    assistant_message,
    children_pending_message,
    compaction_anchors,
    completion_nudge_message,
    leader_land_message,
    leader_nudge_message,
    rehydrate,
    repeated_error,
    summary_message,
    tool_message,
)
from gantry.runtime.tools import (
    INTERRUPTED_RESULT,
    ApprovalPolicy,
    TaskParked,
    TaskStalled,
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


def _delegates(task: Task) -> bool:
    """A delegating agent — a planner, a can_spawn profile, or an autonomous leader.
    The same role predicate ``WorkerConfig.compaction_for`` uses, so the step cap and
    the compaction budget always agree on a task's role (and pick it identically on a
    resume, since it is a pure function of the durable kind/payload)."""
    payload = task.payload
    return (
        task.kind is TaskKind.PLAN
        or bool(payload.get("can_spawn"))
        or bool(payload.get("autonomous_leader"))
    )


def max_steps_for(task: Task, settings: Settings) -> int:
    """The always-finite per-agent step ceiling, by role. An explicit per-task
    ``max_steps`` wins. Otherwise a DELEGATING agent gets a generous but finite budget
    (never unbounded — even an unattended leader must halt); an autonomous-swarm LEAF
    (a non-interactive spawned micro-task) gets a tight one, because a worker handed
    one file + one outcome that needs dozens of steps is stuck; a standalone /
    interactive leaf keeps the default budget. Pure function of the durable kind/
    payload, so a resume selects the identical cap."""
    explicit = task.payload.get("max_steps")
    if explicit:
        return int(explicit)
    if _delegates(task):
        return settings.leader_max_steps
    if task.payload.get("non_interactive"):
        return settings.execute_max_steps
    return settings.default_max_steps


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
    #: How many times this run compacted its history — an observability signal
    #: (frequent compaction => the prefix cache keeps resetting).
    compactions: int = 0

    @property
    def cache_hit_ratio(self) -> float:
        """Fraction of input tokens served from the provider's prompt cache — the
        headline cache-warmth signal (0.0 when nothing was cached)."""
        return self.cache_read_tokens / self.prompt_tokens if self.prompt_tokens else 0.0


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
        compactions=state.compactions,
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
    # An orchestrator (a planner, or a profile that can spawn) coordinates and
    # parks while its children work — it legitimately takes many steps, so it is
    # NOT step-capped as tightly as a leaf, but still finite (see max_steps_for).
    # An autonomous leader has no write tools and exists only to delegate, so the
    # loop budgets how long it may survey before it must start spawning.
    is_leader = bool(payload.get("autonomous_leader"))
    # Optional per-run USD budget (inherited into every task of the run). When
    # this task's own spend crosses it, the loop halts GRACEFULLY at a step
    # boundary (never mid-tool) — the run-level brake that stops a leader fanning
    # out more work lives in the spawn tools.
    budget_usd = payload.get("budget_usd")
    # Always-finite step ceiling (circuit breaker): a delegating agent is no longer
    # unbounded, so a thrashing leader can't loop forever, and a stuck micro-task
    # dies fast. When a delegating agent hits it, we halt GRACEFULLY (below); a leaf
    # raises AgentLoopError so its parent learns it failed.
    delegates = _delegates(task)
    max_steps = max_steps_for(task, settings)
    ctx = ToolContext(
        task_id=task.id,
        workspace=workspace,
        emit_event=partial(_checkpoint, sessions, task),
        sessions=sessions,
    )

    async with session_scope(sessions) as session:
        events = await read_events(session, task.id)
    state = rehydrate(payload, events)
    # An escalation (recorded as an event, never a payload edit) overrides the
    # launch model, so a task handed to a stronger reasoner after stalling keeps
    # that model across every later resume.
    model = state.escalated_model or payload.get("model") or settings.default_model
    if state.resumed:
        logger.info(
            "agent.resumed", task_id=str(task.id), steps=state.steps, messages=len(state.tracked)
        )
    if state.escalated_model:
        logger.info("agent.escalated_model", task_id=str(task.id), model=model)

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
        if state.steps >= max_steps:
            # A delegating agent that burned its (finite) step budget has been
            # thrashing — halt CLEANLY with a report, not a hard FAIL that would
            # read as the orchestration itself erroring. A leaf instead fails
            # (non-retryably, via the caller) so its parent learns it got stuck.
            if delegates:
                logger.info(
                    "agent.step_halt",
                    task_id=str(task.id),
                    steps=state.steps,
                    max_steps=max_steps,
                )
                return _outcome(
                    state,
                    model,
                    f"Halted: reached the step budget ({max_steps}) without finishing. "
                    "Stopping cleanly instead of spending more. Integrate and land any "
                    "usable work; if the goal is too large, re-launch it split smaller.",
                )
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

        # Logical stagnation check, at a step boundary so nothing is interrupted
        # mid-tool. Runs before the LLM call: the whole value is NOT spending
        # another turn on an approach already proven to fail.
        _check_repair_loop(task, state, settings)
        _check_passive_read_loop(task, state, settings, delegates=delegates)

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
        # tracked/events keep full args for recover()/replay. If the provider
        # rejects the prompt as too long, recover by force-compacting and retrying
        # rather than failing the task (a context-overflow is a permanent 400).
        response = await _complete_with_context_recovery(
            sessions, task, state, llm, model, tools, compaction, on_delta
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
            # A leaf worker (not a delegating agent, which has its own guards) that
            # tries to finish right after a failed action is giving up mid-task, not
            # done — nudge it to continue instead of marking a hollow SUCCESS.
            if reminder is None and not delegates:
                reminder = await _completion_guard(sessions, task, state)
            if reminder is not None:
                state.tracked.append(reminder)
                continue
            _check_leader_delivery(task, state, is_leader=is_leader)
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
    # Mirror the rehydrate fold: only the most recent result's error-ness is kept,
    # so the completion guard sees the same signal live and on resume.
    state.last_tool_errored = result.is_error
    await _record_diagnostics(sessions, task, state, tc, result)


async def _record_diagnostics(
    sessions: Sessions,
    task: Task,
    state: AgentState,
    tc: ToolCallRequest,
    result: ToolResult,
) -> None:
    """Checkpoint the structured errors a tool reported, as loop-detector evidence.

    Durable rather than in-memory because the repetition worth catching happens
    across steps AND across attempts: an in-memory counter resets on every crash
    or re-claim, which is exactly when a stuck task gets another chance to be
    stuck. Only ERROR-severity fingerprints are recorded — a build that warns
    identically while genuinely progressing is not looping.
    """
    found = list(result.diagnostics)
    marks = diagnostics.fingerprints(found)
    if not marks:
        return
    await _checkpoint(
        sessions,
        task,
        EventType.DIAGNOSTICS,
        {
            "tool_call_id": tc.id,
            "tool": tc.name,
            "fingerprints": marks,
            "diagnostics": diagnostics.to_payload(found),
        },
    )
    # Must mirror the DIAGNOSTICS fold in state.rehydrate exactly, or a resumed
    # task's evidence diverges from a live one's and the two disagree about
    # whether the task is stuck.
    state.diagnostic_batches.append(marks)
    state.diagnostic_lines.extend(d.render() for d in diagnostics.errors(found))


async def _execute_tool(tools: ToolRegistry, tc: ToolCallRequest, ctx: ToolContext) -> ToolResult:
    malformed = malformed_arguments(tc.arguments)
    if malformed is not None:
        # The model emitted a tool call whose JSON arguments didn't parse (e.g. a
        # stream that truncated them). Report it as a structured diagnostic — same
        # fingerprint each time the same call recurs, so a model stuck emitting the
        # same broken call is stalled/escalated — instead of handing garbage to the
        # tool or letting a KeyError crash the loop.
        diagnostic = diagnostics.Diagnostic(
            file=tc.name,
            line=None,
            column=None,
            message="tool call arguments were not valid JSON — re-emit the call with "
            "complete, well-formed JSON arguments",
            severity=diagnostics.Severity.ERROR,
            code="MalformedToolCall",
            source="tool-validation",
        )
        return ToolResult(
            f"Malformed tool call to {tc.name}: arguments were not valid JSON "
            f"({malformed[:200]}). Re-issue the call with complete JSON arguments.",
            is_error=True,
            diagnostics=(diagnostic,),
        )
    tool = tools.get(tc.name)
    if tool is None:
        # A hallucinated tool is structured like any other failure so it feeds the
        # repair-loop breaker: an agent that keeps calling the same non-existent
        # tool produces the same fingerprint each turn and is stalled/escalated,
        # instead of burning its whole step budget on a tool that will never exist.
        diagnostic = diagnostics.Diagnostic(
            file=tc.name,
            line=None,
            column=None,
            message=f"no tool named {tc.name!r} exists — call only tools in your toolset",
            severity=diagnostics.Severity.ERROR,
            code="UnknownTool",
            source="tool-validation",
        )
        return ToolResult(
            f"Unknown tool: {tc.name}. It is not in your toolset — do not call it again.",
            is_error=True,
            diagnostics=(diagnostic,),
        )
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


def _check_leader_delivery(task: Task, state: AgentState, *, is_leader: bool) -> None:
    """Reject an autonomous leader's premature exit (the "lazy leader fast-exit").

    An autonomous leader has NO write tools — its only way to produce anything is
    to delegate. If it reaches a final answer having spawned zero workers and
    integrated nothing (no merge, no land), it delivered nothing at all: fail it so
    the run gets a clear signal instead of a silent success over an empty result.
    A leader that spawned anything is exempt — it did its job, even if it is now
    reporting the children's failure. Pure function of durable state, so a resume
    decides identically.
    """
    if not is_leader:
        return
    if state.count_tool_calls(*SPAWN_TOOL_NAMES, "merge_child_branches", "land_branch") == 0:
        logger.warning("agent.leader_empty_exit", task_id=str(task.id))
        raise AgentLoopError(
            "autonomous leader tried to finish without delegating any work: it "
            "spawned no workers and integrated nothing. A leader must split the goal "
            "into micro-tasks and spawn workers; finishing empty delivers nothing."
        )


def _check_passive_read_loop(
    task: Task, state: AgentState, settings: Any, *, delegates: bool
) -> None:
    """Break a passive read loop: fail a leaf worker that only ever reads.

    A leaf (non-delegating) worker is handed one file and one outcome. If it makes
    many read-only calls (read_file/glob/grep/...) and not a SINGLE productive one
    (edit/write/bash/commit), it is circling the problem instead of solving it —
    stuck in a way the repair-loop breaker (which needs a recurring *diagnostic*)
    never sees, because reading never errors. Fail it fast so its leader learns and
    can respawn with a sharper task, well before it grinds to the step cap.

    A delegating agent surveys legitimately before spawning (the leader survey guard
    governs that), so this never applies to it. A pure function of durable state, so
    a resume decides identically. ``passive_read_max`` <= 0 disables it.
    """
    limit = int(getattr(settings, "passive_read_max", 0) or 0)
    if limit <= 0 or delegates:
        return
    if state.count_tool_calls(*PRODUCTIVE_TOOL_NAMES):
        return  # it has done real work — not a passive reader
    reads = state.count_tool_calls(*READ_ONLY_TOOL_NAMES)
    if reads < limit:
        return
    logger.warning("agent.passive_read_loop_detected", task_id=str(task.id), reads=reads)
    raise AgentLoopError(
        f"passive read loop: {reads} read-only tool calls with no edit, write, "
        "command, or delegation. A worker that only reads is stuck — failing so the "
        "leader can respawn with a sharper task."
    )


def _check_repair_loop(task: Task, state: AgentState, settings: Any) -> None:
    """Break a repair loop: stall the task rather than fund another identical try.

    Fires when one error fingerprint recurs across the recent diagnostic window
    (see ``state.repeated_error``). The task is then routed for escalation — a
    stronger reasoning model, with the structured error history as its opening
    context — instead of the current model burning steps on the edit it has
    already made three times.

    Escalation happens at most once. A task that stalls AGAIN under the stronger
    model is not going to be fixed by a third opinion: it fails terminally with
    the offending error in the message, which is a far better signal for the
    leader (respawn with a different decomposition) than a task quietly grinding
    to its step cap.
    """
    fingerprint = repeated_error(state.diagnostic_batches)
    if fingerprint is None:
        return
    history = _diagnostic_history(state)
    if state.escalations >= _MAX_ESCALATIONS:
        raise AgentLoopError(
            "repair loop persisted after escalation: the same error keeps "
            f"recurring ({fingerprint}). Last errors:\n" + "\n".join(history)
        )
    logger.warning(
        "agent.repair_loop_detected",
        task_id=str(task.id),
        fingerprint=fingerprint,
        batches=len(state.diagnostic_batches),
    )
    raise TaskStalled(
        fingerprint=fingerprint,
        history=history,
        model=getattr(settings, "escalation_model", None),
    )


#: A single escalation. Beyond this the problem is the plan, not the model.
_MAX_ESCALATIONS = 1
#: How many recent distinct errors to hand the escalated run.
_HISTORY_LIMIT = 10


def _diagnostic_history(state: AgentState) -> list[str]:
    """Recent distinct error renderings, newest last — the escalation's evidence."""
    seen: set[str] = set()
    lines: list[str] = []
    for event_line in state.diagnostic_lines[-_HISTORY_LIMIT:]:
        if event_line in seen:
            continue
        seen.add(event_line)
        lines.append(event_line)
    return lines


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


#: Backstop so a worker that keeps failing-then-giving-up still terminates rather
#: than looping on the nudge. One nudge is enough to recover a reasoning model that
#: simply forgot to emit its next tool call; past that a stuck worker may finish.
_MAX_COMPLETION_NUDGES = 1


async def _completion_guard(
    sessions: Sessions, task: Task, state: AgentState
) -> TrackedMessage | None:
    """Reject a leaf worker's premature finish RIGHT AFTER a failed action.

    A worker that ends its turn with no tool call while its last action errored is
    almost always giving up mid-task — a reasoning model narrates the fix it means
    to make next, then omits the actual tool call, and the loop would read that as
    a completed run and mark a hollow SUCCESS. Durably nudge it to continue (fold
    like the other reminders). ``None`` means it may finish: a clean finish (its
    last action did not error), or the nudge budget is spent so a genuinely stuck
    worker still terminates.
    """
    if not state.last_tool_errored:
        return None
    if state.completion_nudges >= _MAX_COMPLETION_NUDGES:
        return None
    seq = await _checkpoint(sessions, task, EventType.COMPLETION_NUDGE, {})
    state.completion_nudges += 1
    logger.info("agent.completion_guard", task_id=str(task.id), nudges=state.completion_nudges)
    return TrackedMessage(seq, completion_nudge_message())


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
    state.compactions += 1  # mirrors the rehydrate fold in state.py
    logger.info(
        "agent.compacted",
        task_id=str(task.id),
        summarized=result.summarized_messages,
        kept=len(result.kept_seqs),
    )


#: How many times one step may recover from a provider context-overflow by
#: force-compacting and retrying before it gives up and surfaces the error.
_MAX_CONTEXT_RECOVERIES = 2


def _is_context_overflow(exc: Exception) -> bool:
    """Whether ``exc`` is a provider "prompt too long" rejection. LiteLLM
    normalizes these to ``ContextWindowExceededError``; the message/status check is
    a defensive fallback for providers it doesn't map."""
    if type(exc).__name__ == "ContextWindowExceededError":
        return True
    if getattr(exc, "status_code", None) == 400:
        msg = str(exc).lower()
        return "context" in msg and ("length" in msg or "window" in msg or "token" in msg)
    return False


async def _force_compact_once(
    sessions: Sessions,
    task: Task,
    state: AgentState,
    llm: LLMClient,
    model: str,
    config: CompactionConfig,
) -> bool:
    """Force one compaction regardless of the token estimate. Returns whether it
    actually shrank the history (False when there is nothing left to compact)."""
    projected = state.projected_messages()
    plan = plan_compaction(state.tracked, config, projected, force=True)
    if plan is None:
        return False
    before = len(state.tracked)
    await _apply_compaction(sessions, task, state, llm, model, plan, projected)
    return len(state.tracked) < before


async def _complete_with_context_recovery(
    sessions: Sessions,
    task: Task,
    state: AgentState,
    llm: LLMClient,
    model: str,
    tools: ToolRegistry,
    compaction: CompactionConfig | None,
    on_delta: DeltaSink,
) -> LLMResponse:
    """Call the model, recovering from a context-overflow rejection by
    force-compacting the history and retrying (bounded). Any other error, or an
    overflow we can no longer shrink, propagates to the worker's retry path."""
    for attempt in range(_MAX_CONTEXT_RECOVERIES + 1):
        try:
            return await llm.complete(
                model=model,
                messages=state.projected_messages(),
                tools=tools.schemas(),
                on_delta=on_delta,
            )
        except Exception as exc:
            recoverable = (
                attempt < _MAX_CONTEXT_RECOVERIES
                and compaction is not None
                and _is_context_overflow(exc)
            )
            if not recoverable:
                raise
            assert compaction is not None  # narrowed by `recoverable`
            logger.warning(
                "agent.context_overflow_recovery", task_id=str(task.id), attempt=attempt + 1
            )
            if not await _force_compact_once(sessions, task, state, llm, model, compaction):
                raise  # nothing left to shrink — surface the original rejection
    raise AssertionError("unreachable")  # pragma: no cover


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
