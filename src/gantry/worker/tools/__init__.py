"""Toolset assembly for worker agents, selected by task kind."""

from __future__ import annotations

from gantry.runtime.tools import ToolRegistry
from gantry.worker.git import GitAuth
from gantry.worker.tools.bash import BashTool
from gantry.worker.tools.files import EditFileTool, ListDirTool, ReadFileTool, WriteFileTool
from gantry.worker.tools.gittool import GitCommitPushTool
from gantry.worker.tools.orchestration import build_planner_registry

__all__ = ["build_coding_registry", "build_planner_registry"]


def build_coding_registry(auth: GitAuth | None = None) -> ToolRegistry:
    registry = ToolRegistry(
        [BashTool(), ReadFileTool(), WriteFileTool(), EditFileTool(), ListDirTool()]
    )
    if auth is not None:
        registry.register(GitCommitPushTool(auth))
    return registry
