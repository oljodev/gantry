"""Context-control invariants: write/edit arg elision is a pure send-time
projection (never mutates history), and size-aware / emergency compaction bound
the per-step input while keeping the [system, goal] anchors verbatim."""

from __future__ import annotations

import copy
import json
from pathlib import Path

from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.runtime.compaction import CompactionConfig, plan_compaction
from gantry.runtime.loop import run_agent_task
from gantry.runtime.state import (
    AgentState,
    TrackedMessage,
    assistant_message,
    compaction_anchors,
    elide_resolved_writes,
    initial_messages,
    tool_message,
)
from gantry.runtime.tools import ToolRegistry
from gantry.worker.tools.files import ReadFileTool, WriteFileTool

from .fakes import ScriptedLLM, final_response, response_with_tool_call
from .test_agent_loop import enqueue_agent_task

Sessions = async_sessionmaker[AsyncSession]


def _assistant_call(call_id: str, name: str, arguments: dict) -> dict:
    return assistant_message(None, [{"id": call_id, "name": name, "arguments": arguments}])


def test_elision_replaces_resolved_write_body_without_mutating_input() -> None:
    body = "x" * 5000
    msg = _assistant_call("w1", "write_file", {"path": "a.py", "content": body})
    original = copy.deepcopy(msg)

    projected = elide_resolved_writes(msg, {"w1"})

    # The input is NEVER mutated (pure projection) ...
    assert msg == original
    # ... the sent copy drops the body and carries an unambiguous annotation.
    args = json.loads(projected["tool_calls"][0]["function"]["arguments"])
    assert "content" not in args
    assert args["path"] == "a.py"
    assert args["_gantry_elided"].startswith("[gantry:") and "5000 chars" in args["_gantry_elided"]


def test_elision_only_touches_resolved_write_and_edit_calls() -> None:
    write = _assistant_call("w1", "write_file", {"path": "a", "content": "z" * 100})
    # Unresolved -> untouched (recover()/re-exec must see the real args).
    assert elide_resolved_writes(write, set()) is write
    # A non-write tool -> untouched even when resolved.
    bash = _assistant_call("b1", "bash", {"command": "ls"})
    assert elide_resolved_writes(bash, {"b1"}) is bash
    # edit_file old_str/new_str are elided too.
    edit = _assistant_call(
        "e1", "edit_file", {"path": "a", "old_str": "p" * 80, "new_str": "q" * 90}
    )
    args = json.loads(elide_resolved_writes(edit, {"e1"})["tool_calls"][0]["function"]["arguments"])
    assert "old_str" not in args and "new_str" not in args and "170 chars" in args["_gantry_elided"]


def test_projected_messages_shrinks_only_resolved_writes() -> None:
    state = AgentState(tracked=initial_messages({"goal": "do it"}))
    state.tracked.append(
        TrackedMessage(1, _assistant_call("w1", "write_file", {"path": "a", "content": "y" * 9000}))
    )
    state.tracked.append(TrackedMessage(2, tool_message("w1", "wrote a")))
    state.tracked.append(
        TrackedMessage(3, _assistant_call("w2", "write_file", {"path": "b", "content": "y" * 9000}))
    )
    state.resolved_tool_ids = {"w1"}  # w2 still pending
    # tracked = [system(0), goal(1), write w1(2), tool(3), write w2(4)]

    projected = state.projected_messages()
    # tracked is untouched; only the resolved write is elided in the projection.
    assert "y" * 9000 in state.tracked[2].message["tool_calls"][0]["function"]["arguments"]
    assert "_gantry_elided" in projected[2]["tool_calls"][0]["function"]["arguments"]
    assert "y" * 9000 in projected[4]["tool_calls"][0]["function"]["arguments"]  # pending kept


def _pair(i: int, name: str, args: dict) -> list[TrackedMessage]:
    cid = f"c{i}"
    return [
        TrackedMessage(2 * i, _assistant_call(cid, name, args)),
        TrackedMessage(2 * i + 1, tool_message(cid, "ok")),
    ]


def test_plan_compaction_size_aware_bounds_the_tail() -> None:
    # 1 char per token via count; a small recent-token budget keeps a small tail.
    tracked = initial_messages({"goal": "g"})
    for i in range(1, 8):  # 7 steps -> 14 messages after the 2 anchors
        tracked += _pair(i, "read_file", {"path": f"f{i}"})
    cfg = CompactionConfig(
        max_context_tokens=5,
        keep_recent_messages=2,
        keep_recent_tokens=2,
        token_counter=lambda msgs: len(msgs),
    )
    plan = plan_compaction(tracked, cfg)
    assert plan is not None
    # Cut lands on an assistant boundary past the anchors, leaving a bounded tail.
    assert tracked[plan.cut_index].message["role"] == "assistant" and plan.cut_index > 2
    assert len(tracked) - plan.cut_index <= 3  # tail stays small (budget 2, floor 2)


def test_plan_compaction_force_targets_a_minimal_tail() -> None:
    tracked = initial_messages({"goal": "g"})
    for i in range(1, 6):
        tracked += _pair(i, "read_file", {"path": f"f{i}"})
    # Each message is large relative to the force budget (hard_ceiling//4), so the
    # emergency cut keeps only the newest complete step.
    cfg = CompactionConfig(max_context_tokens=1000, token_counter=lambda msgs: 500 * len(msgs))
    plan = plan_compaction(tracked, cfg, force=True)  # ignores the (unmet) threshold
    assert plan is not None and plan.cut_index == max(
        i for i, t in enumerate(tracked) if t.message["role"] == "assistant" and i > 2
    )


def test_anchors_are_system_and_goal() -> None:
    tracked = initial_messages({"system_prompt": "SYS", "goal": "GOAL"})
    tracked += _pair(1, "read_file", {"path": "f"})
    anchors = compaction_anchors(tracked)
    assert [m.message["content"] for m in anchors] == ["SYS", "GOAL"]


async def test_write_body_is_elided_from_the_next_send(db: Sessions, tmp_path: Path) -> None:
    task = await enqueue_agent_task(db)
    big = "Z" * 60_000  # ~15k tokens of file body
    llm = ScriptedLLM(
        [
            response_with_tool_call("w1", "write_file", {"path": "big.py", "content": big}),
            response_with_tool_call("r1", "read_file", {"path": "big.py"}),
            final_response("done"),
        ]
    )
    outcome = await run_agent_task(
        db, task, llm, ToolRegistry([WriteFileTool(), ReadFileTool()]), workspace=tmp_path
    )
    assert outcome.final_text == "done"

    # The SECOND send (after w1 resolved) must carry the elided placeholder, not the
    # 60k body — that is what stops per-step bloat and per-step compaction/cache thrash.
    second = json.dumps(llm.calls[1]["messages"])
    assert "ZZZZ" not in second
    assert "_gantry_elided" in second
    # The body is still on disk and in the durable history (execution truth).
    assert (tmp_path / "big.py").read_text() == big
