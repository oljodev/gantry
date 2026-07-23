"""The Autonomous Leader (swarm master) prompt trains the orchestrator to
delegate in tiny parallel micro-tasks and gate on a sequential qa-reviewer."""

from __future__ import annotations

from gantry.runtime.state import AUTONOMOUS_LEADER_PROMPT, PLANNER_SYSTEM_PROMPT


def test_leader_prompt_encodes_the_swarm_disciplines() -> None:
    prompt = AUTONOMOUS_LEADER_PROMPT.lower()
    # Micro-task delegation for parallelism.
    assert "micro-task" in prompt
    assert "one function" in prompt or "one file" in prompt
    # Tiny, file-isolated payloads.
    assert "self-contained" in prompt and "tiny" in prompt
    # Batch-spawn then a single wait (the durable spawn/wait machinery).
    assert "spawn_subtask" in prompt and "wait_for_children" in prompt
    # Sequential quality control after the parallel workers.
    assert "qa-reviewer" in prompt


def test_planner_prompt_is_the_leader_prompt() -> None:
    # The orchestrator config is embedded under the historical name so every
    # existing leader path (API launch, spawn_subtask, team planner nodes) uses it.
    assert PLANNER_SYSTEM_PROMPT == AUTONOMOUS_LEADER_PROMPT
