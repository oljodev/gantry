"""Toolset assembly for worker agents.

Every agent gets the coding toolset (files, search, bash, web, ask_user) and a
workspace. An agent whose profile allows delegation (``can_spawn``, or a task
of kind ``plan``) additionally gets the orchestration tools, so a coder can
spawn a reviewer, wait for it, and act on its report — the hybrid that the old
disjoint planner/coder split made impossible.
"""

from __future__ import annotations

from gantry.runtime.tools import Tool, ToolRegistry
from gantry.teams import TeamNode
from gantry.worker.git import GitAuth
from gantry.worker.merge import ConflictResolver
from gantry.worker.tools.ask import AskUserTool
from gantry.worker.tools.bash import BashTool
from gantry.worker.tools.copilot import CreateSkillTool, ProposeSkillTool, ProposeTreeTool
from gantry.worker.tools.files import EditFileTool, ListDirTool, ReadFileTool, WriteFileTool
from gantry.worker.tools.gittool import GitCommitPushTool
from gantry.worker.tools.integrate import LandBranchTool, MergeChildBranchesTool
from gantry.worker.tools.orchestration import (
    DEFAULT_MAX_REPAIR_FAILURES,
    DEFAULT_MAX_SUBTASKS,
    DEFAULT_RUN_TASK_CEILING,
    build_planner_registry,
    orchestration_tools,
)
from gantry.worker.tools.search import GlobTool, GrepTool
from gantry.worker.tools.web import WebFetchTool, WebSearchTool

__all__ = ["build_coding_registry", "build_copilot_registry", "build_planner_registry"]


def build_copilot_registry(kind: str) -> ToolRegistry:
    """Restricted toolset for a co-pilot task. The tree co-pilot can also author
    skills for the team (create_skill), which is gated behind user approval."""
    if kind == "tree":
        return ToolRegistry([ProposeTreeTool(), CreateSkillTool(), AskUserTool()])
    propose: Tool = ProposeSkillTool()
    return ToolRegistry([propose, AskUserTool()])


def _add_delegation_tools(
    registry: ToolRegistry,
    max_subtasks: int,
    team: TeamNode | None,
    auth: GitAuth | None,
    trunk_branch: str | None,
    base_branch: str | None,
    conflict_resolver: ConflictResolver | None,
    *,
    max_repair_failures: int,
    run_task_ceiling: int,
) -> None:
    for tool in orchestration_tools(
        max_subtasks,
        team=team,
        max_repair_failures=max_repair_failures,
        run_task_ceiling=run_task_ceiling,
    ):
        registry.register(tool)
    # A repo-backed leader can integrate the branches its workers push, then land
    # the validated result on main so the work reaches the default branch.
    if auth is not None and trunk_branch is not None:
        registry.register(MergeChildBranchesTool(auth, trunk_branch, conflict_resolver))
        registry.register(LandBranchTool(auth, base_branch))


def build_coding_registry(
    auth: GitAuth | None = None,
    *,
    can_spawn: bool = False,
    leader: bool = False,
    max_subtasks: int = DEFAULT_MAX_SUBTASKS,
    max_repair_failures: int = DEFAULT_MAX_REPAIR_FAILURES,
    run_task_ceiling: int = DEFAULT_RUN_TASK_CEILING,
    team: TeamNode | None = None,
    trunk_branch: str | None = None,
    base_branch: str | None = None,
    conflict_resolver: ConflictResolver | None = None,
) -> ToolRegistry:
    if leader:
        # A pure Autonomous Leader surveys read-only and delegates ALL writing.
        # It deliberately has no write/edit/bash/commit tools, so it *cannot* do
        # the work itself — its only way to change code is to spawn workers.
        registry = ToolRegistry(
            [ReadFileTool(), ListDirTool(), GlobTool(), GrepTool(), AskUserTool()]
        )
        _add_delegation_tools(
            registry,
            max_subtasks,
            team,
            auth,
            trunk_branch,
            base_branch,
            conflict_resolver,
            max_repair_failures=max_repair_failures,
            run_task_ceiling=run_task_ceiling,
        )
        return registry

    registry = ToolRegistry(
        [
            BashTool(),
            ReadFileTool(),
            WriteFileTool(),
            EditFileTool(),
            ListDirTool(),
            GlobTool(),
            GrepTool(),
            WebSearchTool(),
            WebFetchTool(),
            AskUserTool(),
        ]
    )
    if auth is not None:
        registry.register(GitCommitPushTool(auth))
    if can_spawn:
        _add_delegation_tools(
            registry,
            max_subtasks,
            team,
            auth,
            trunk_branch,
            base_branch,
            conflict_resolver,
            max_repair_failures=max_repair_failures,
            run_task_ceiling=run_task_ceiling,
        )
    return registry
