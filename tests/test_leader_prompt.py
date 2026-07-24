"""The Autonomous Leader (swarm master) prompt trains the orchestrator to
delegate in tiny parallel micro-tasks and gate on a sequential qa-reviewer."""

from __future__ import annotations

from gantry.runtime.state import AUTONOMOUS_LEADER_PROMPT, PLANNER_SYSTEM_PROMPT


def test_leader_prompt_encodes_the_swarm_disciplines() -> None:
    prompt = AUTONOMOUS_LEADER_PROMPT.lower()
    # Survey the codebase before delegating — read, don't plan blind.
    assert "read_file" in prompt and "survey" in prompt
    # Survey comes before the delegation section — read before delegating.
    assert prompt.index("survey") < prompt.index("delegate in micro-task")
    # ask_user is for product decisions, not facts it can read.
    assert "ask_user" in prompt
    # Micro-task delegation for parallelism.
    assert "micro-task" in prompt
    assert "one function" in prompt or "one file" in prompt
    # Tiny, file-isolated payloads.
    assert "self-contained" in prompt and "tiny" in prompt
    # Batch-spawn then a single wait (the durable spawn/wait machinery).
    assert "spawn_subtask" in prompt and "wait_for_children" in prompt
    # Integrate the pushed branches (merge + conflict resolution) before QA...
    assert "merge_child_branches" in prompt
    assert "commit and push" in prompt  # workers must push or there's nothing to merge
    # ...then sequential quality control on the integrated staging branch.
    assert "qa-reviewer" in prompt
    assert prompt.index("merge_child_branches") < prompt.index("qa-reviewer")
    # ...and finally landing the validated result on main is the last step.
    assert "land_branch" in prompt and "land on main" in prompt
    assert prompt.index("qa-reviewer") < prompt.index("land_branch")


def test_leader_prompt_forbids_designing_the_solution_itself() -> None:
    # The recurring failure mode: the leader burns minutes designing the whole
    # decomposition (line counts, which method goes where) instead of handing a
    # file and an outcome to a worker. The prompt must forbid that explicitly.
    prompt = AUTONOMOUS_LEADER_PROMPT.lower()
    assert "router, not a designer" in prompt
    assert "delegate outcomes, not implementations" in prompt
    # Name the specific over-planning tells it must not do.
    assert "line count" in prompt
    # Delegate a boundary + outcome, and let the worker design the "how".
    assert "file boundary" in prompt and "never dictate how" in prompt


def test_leader_prompt_tiers_the_child_model_by_cost() -> None:
    # Children should run on the cheapest model that fits, not inherit the
    # leader's expensive model for mechanical work.
    prompt = AUTONOMOUS_LEADER_PROMPT.lower()
    assert "match the model to the work" in prompt
    assert "cheapest model" in prompt
    assert "spawn_subtask" in prompt and "`model`" in prompt


def test_leader_prompt_guards_against_collisions_and_thrash() -> None:
    # The hour-long chess-split failure: coupled work fanned out to blind
    # workers collided at merge, then the leader thrashed with fixup rounds.
    prompt = AUTONOMOUS_LEADER_PROMPT.lower()
    # Disjoint file ownership + a single owner for shared glue.
    assert "disjoint" in prompt and "glue" in prompt
    # Sequence coupled steps instead of parallelizing them.
    assert "not parallel" in prompt
    # Anti-thrash: converge, targeted fixes, don't redo whole modules.
    assert "converge" in prompt and "thrash" in prompt


def test_planner_prompt_is_the_leader_prompt() -> None:
    # The orchestrator config is embedded under the historical name so every
    # existing leader path (API launch, spawn_subtask, team planner nodes) uses it.
    assert PLANNER_SYSTEM_PROMPT == AUTONOMOUS_LEADER_PROMPT
